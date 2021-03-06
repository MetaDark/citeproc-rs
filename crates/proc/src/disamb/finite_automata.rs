// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Copyright © 2019 Corporation for Digital Scholarship

use crate::db::IrDatabase;
use citeproc_io::output::{markup::Markup, OutputFormat};
use petgraph::dot::Dot;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use salsa::{InternId, InternKey};
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt::Debug;

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct Edge(u32);

// XXX(pandoc): maybe force this to be a string and coerce pandoc output into a string
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EdgeData<O: OutputFormat = Markup> {
    Output(O::Output),

    // The rest are synchronised with fields on CiteContext and IR.
    Locator,
    LocatorLabel,

    /// TODO: add a parameter to Dfa::accepts_data to supply the actual year suffix for the particular reference.
    YearSuffix,

    /// Not for DFA matching, must be turned into YearSuffix via `RefIR::keep_first_ysh` before DFA construction
    YearSuffixExplicit,

    CitationNumber,
    CitationNumberLabel,

    // TODO: treat this specially? Does it help you disambiguate back-referencing cites?
    Frnn,
    FrnnLabel,
}

use std::hash::{Hash, Hasher};

/// Have to implement Hash ourselves because of the blanket O on EdgeData<O>>. This is basically
/// what the derive macro spits out.
impl Hash for EdgeData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        use std::mem::discriminant;
        match self {
            EdgeData::Output(ref outp) => {
                ::core::hash::Hash::hash(&discriminant(self), state);
                ::core::hash::Hash::hash(&(*outp), state)
            }
            _ => ::core::hash::Hash::hash(&discriminant(self), state),
        }
    }
}

impl Edge {
    // Adding this method is often convenient, since you can then
    // write `path.lookup(db)` to access the data, which reads a bit better.
    pub fn lookup(self, db: &impl IrDatabase) -> EdgeData {
        IrDatabase::lookup_edge(db, self)
    }
}

impl InternKey for Edge {
    fn from_intern_id(v: InternId) -> Self {
        Edge(u32::from(v))
    }
    fn as_intern_id(&self) -> InternId {
        InternId::from(self.0)
    }
}

// impl Debug for EdgeData {
//     fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
//         write!(f, "{}", self.0)
//     }
// }

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum NfaEdge {
    Epsilon,
    Token(Edge),
}

impl From<Edge> for NfaEdge {
    fn from(edge: Edge) -> Self {
        NfaEdge::Token(edge)
    }
}

pub type NfaGraph = Graph<(), NfaEdge>;
pub type DfaGraph = Graph<(), Edge>;

fn epsilon_closure(nfa: &NfaGraph, closure: &mut BTreeSet<NodeIndex>) {
    let mut work: Vec<_> = closure.iter().cloned().collect();
    while !work.is_empty() {
        let s = work.pop().unwrap();
        for edge in nfa.edges(s) {
            let is_epsilon = *edge.weight() == NfaEdge::Epsilon;
            let target = edge.target();
            if is_epsilon && !closure.contains(&target) {
                work.push(target);
                closure.insert(target);
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Nfa {
    pub graph: NfaGraph,
    pub accepting: BTreeSet<NodeIndex>,
    pub start: BTreeSet<NodeIndex>,
}

#[derive(Clone)]
pub struct Dfa {
    pub graph: DfaGraph,
    pub accepting: BTreeSet<NodeIndex>,
    pub start: NodeIndex,
}

// This is not especially useful for comparing Dfas, but it allows Salsa to cache it as opposed to
// not at all.
impl Eq for Dfa {}
impl PartialEq for Dfa {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

#[derive(Debug)]
enum DebugNode {
    Node,
    Start,
    Accepting,
    StartAndAccepting,
}

impl Dfa {
    pub fn debug_graph(&self, db: &impl IrDatabase) -> String {
        let g = self.graph.map(
            |node, _| {
                let cont = self.accepting.contains(&node);
                if node == self.start && cont {
                    DebugNode::StartAndAccepting
                } else if node == self.start {
                    DebugNode::Start
                } else if cont {
                    DebugNode::Accepting
                } else {
                    DebugNode::Node
                }
            },
            |_, edge| db.lookup_edge(*edge),
        );
        format!("{:?}", Dot::with_config(&g, &[]))
    }
}

impl Nfa {
    pub fn new() -> Self {
        Nfa::default()
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.accepting
    }

    pub fn add_complete_sequence(&mut self, tokens: Vec<Edge>) {
        let mut cursor = self.graph.add_node(());
        self.start.insert(cursor);
        for token in tokens {
            let next = self.graph.add_node(());
            self.graph.add_edge(cursor, next, NfaEdge::Token(token));
            cursor = next;
        }
        self.accepting.insert(cursor);
    }

    /// A Names block, for instance, is given start & end nodes, and simply fills in a segment a
    /// few times with increasing given-name counts etc.
    pub fn add_sequence_between(&mut self, a: NodeIndex, b: NodeIndex, tokens: Vec<Edge>) {
        let mut cursor = self.graph.add_node(());
        self.graph.add_edge(a, cursor, NfaEdge::Epsilon);
        for token in tokens {
            let next = self.graph.add_node(());
            self.graph.add_edge(cursor, next, NfaEdge::Token(token));
            cursor = next;
        }
        self.graph.add_edge(cursor, b, NfaEdge::Epsilon);
    }

    pub fn brzozowski_minimise(mut self: Nfa) -> Dfa {
        use std::mem;
        // reverse
        let rev1 = {
            self.graph.reverse();
            mem::swap(&mut self.start, &mut self.accepting);
            self
        };
        let mut dfa1 = to_dfa(&rev1);
        let rev2 = {
            dfa1.graph.reverse();
            let mut start_set = BTreeSet::new();
            start_set.insert(dfa1.start);
            Nfa {
                graph: dfa1.graph.map(|_, _| (), |_, e| NfaEdge::Token(*e)),
                accepting: start_set,
                start: dfa1.accepting,
            }
        };
        to_dfa(&rev2)
    }
}

use std::fmt::{self, Formatter};

impl Debug for Dfa {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "start:{:?}\naccepting:{:?}\n{:?}\n---\n",
            &self.start,
            &self.accepting,
            Dot::with_config(&self.graph, &[])
        )
    }
}

// TODO: find the quotient automaton of the resulting DFA?
// Use Brzozowski's double-reversal algorithm
pub fn to_dfa(nfa: &Nfa) -> Dfa {
    let mut dfa = DfaGraph::new();

    let mut work = Vec::new();
    let mut start_set = nfa.start.clone();
    epsilon_closure(&nfa.graph, &mut start_set);
    let dfa_start_node = dfa.add_node(());
    let mut dfa_accepting = BTreeSet::new();
    for s in start_set.iter() {
        if nfa.accepting.contains(s) {
            dfa_accepting.insert(dfa_start_node);
            break;
        }
    }
    work.push((start_set.clone(), dfa_start_node));

    let mut dfa_states = HashMap::new();
    dfa_states.insert(start_set, dfa_start_node);

    while !work.is_empty() {
        let (dfa_state, current_node) = work.pop().unwrap();
        let mut by_edge_weight = HashMap::<Edge, BTreeSet<NodeIndex>>::new();
        for nfa_node in dfa_state {
            for edge in nfa.graph.edges(nfa_node) {
                let &weight = edge.weight();
                let target = edge.target();
                if let NfaEdge::Token(t) = weight {
                    by_edge_weight
                        .entry(t)
                        .and_modify(|set| {
                            set.insert(target);
                        })
                        .or_insert_with(|| {
                            let mut set = BTreeSet::new();
                            set.insert(target);
                            set
                        });
                }
            }
        }
        for (k, mut set) in by_edge_weight.drain() {
            epsilon_closure(&nfa.graph, &mut set);
            if !dfa_states.contains_key(&set) {
                let node = dfa.add_node(());
                for s in set.iter() {
                    if nfa.accepting.contains(s) {
                        dfa_accepting.insert(node);
                        break;
                    }
                }
                dfa_states.insert(set.clone(), node);
                dfa.add_edge(current_node, node, k);
                work.push((set, node));
            } else {
                let &node = dfa_states.get(&set).unwrap();
                dfa.add_edge(current_node, node, k);
            }
        }
    }
    Dfa {
        graph: dfa,
        start: dfa_start_node,
        accepting: dfa_accepting,
    }
}

impl Dfa {
    pub fn accepts_data(&self, db: &impl IrDatabase, data: &[EdgeData]) -> bool {
        let mut cursors = Vec::new();
        cursors.push((self.start, None, data));
        while !cursors.is_empty() {
            let (cursor, prepended, chunk) = cursors.pop().unwrap();
            let first = prepended.as_ref().or_else(|| chunk.get(0));
            if first == None && self.accepting.contains(&cursor) {
                // we did it!
                return true;
            }
            if let Some(token) = first {
                for edge in self.graph.edges(cursor) {
                    let weight = db.lookup_edge(*edge.weight());
                    let target = edge.target();
                    use std::cmp::min;
                    match (&weight, token) {
                        // TODO: add an output check that EdgeData::YearSuffix contains the RIGHT
                        (w, t) if w == t => {
                            cursors.push((target, None, &chunk[min(1, chunk.len())..]));
                        }
                        (EdgeData::Output(w), EdgeData::Output(t)) => {
                            if w == t {
                                cursors.push((target, None, &chunk[min(1, chunk.len())..]));
                            } else if t.starts_with(w) {
                                let next = if prepended.is_some() {
                                    // already have split this one
                                    0
                                } else {
                                    1
                                };
                                let t_rest = &t[w.len()..];
                                let c_rest = &chunk[min(next, chunk.len())..];
                                cursors.push((
                                    target,
                                    Some(EdgeData::Output(t_rest.into())),
                                    c_rest,
                                ));
                            }
                        }
                        _ => {} // Don't continue down this path
                    }
                }
            }
        }
        false
    }

    pub fn accepts(&self, tokens: &[Edge]) -> bool {
        let mut cursor = self.start;
        for token in tokens {
            let mut found = false;
            for neighbour in self.graph.neighbors(cursor) {
                let weight = self
                    .graph
                    .find_edge(cursor, neighbour)
                    .and_then(|e| self.graph.edge_weight(e))
                    .map(|w| w == token);
                if let Some(true) = weight {
                    cursor = neighbour;
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }
        self.accepting.contains(&cursor)
    }
}

#[test]
fn nfa() {
    let andy = Edge(1);
    let reuben = Edge(2);
    let peters = Edge(3);
    let comma = Edge(4);
    let twenty = Edge(5);

    let nfa = {
        let mut nfa = NfaGraph::new();
        let initial = nfa.add_node(());
        let forwards1 = nfa.add_node(());
        let backwards1 = nfa.add_node(());
        let backwards2 = nfa.add_node(());
        let target = nfa.add_node(());
        let abc = nfa.add_node(());
        let acc = nfa.add_node(());
        nfa.add_edge(initial, forwards1, reuben.into());
        nfa.add_edge(forwards1, target, peters.into());
        nfa.add_edge(initial, backwards1, peters.into());
        nfa.add_edge(backwards1, backwards2, comma.into());
        nfa.add_edge(backwards2, target, reuben.into());
        nfa.add_edge(initial, target, peters.into());
        nfa.add_edge(target, abc, comma.into());
        nfa.add_edge(abc, acc, twenty.into());
        let mut accepting = BTreeSet::new();
        accepting.insert(acc);
        let mut start = BTreeSet::new();
        start.insert(initial);
        Nfa {
            graph: nfa,
            accepting,
            start,
        }
    };

    let nfa2 = {
        let mut nfa = NfaGraph::new();
        let initial = nfa.add_node(());
        let forwards1 = nfa.add_node(());
        let backwards1 = nfa.add_node(());
        let backwards2 = nfa.add_node(());
        let target = nfa.add_node(());
        let abc = nfa.add_node(());
        let acc = nfa.add_node(());
        nfa.add_edge(initial, forwards1, andy.into());
        nfa.add_edge(forwards1, target, peters.into());
        nfa.add_edge(initial, backwards1, peters.into());
        nfa.add_edge(backwards1, backwards2, comma.into());
        nfa.add_edge(backwards2, target, andy.into());
        nfa.add_edge(initial, target, peters.into());
        nfa.add_edge(target, abc, comma.into());
        nfa.add_edge(abc, acc, twenty.into());
        let mut accepting = BTreeSet::new();
        accepting.insert(acc);
        let mut start = BTreeSet::new();
        start.insert(initial);
        Nfa {
            graph: nfa,
            accepting,
            start,
        }
    };

    let dfa = to_dfa(&nfa);
    let dfa2 = to_dfa(&nfa2);

    let dfa_brz = nfa.brzozowski_minimise();
    let dfa2_brz = nfa2.brzozowski_minimise();

    println!("{:?}", dfa.start);
    println!("dfa {:?}", Dot::with_config(&dfa.graph, &[]));
    println!("dfa2 {:?}", Dot::with_config(&dfa2.graph, &[]));
    println!("dfa_brz {:?}", Dot::with_config(&dfa2_brz.graph, &[]));
    println!("dfa2_brz {:?}", Dot::with_config(&dfa2_brz.graph, &[]));

    let test_dfa = |dfa: &Dfa| {
        assert!(dfa.accepts(&[peters, comma, twenty]));
        assert!(dfa.accepts(&[reuben, peters, comma, twenty]));
        assert!(dfa.accepts(&[peters, comma, reuben, comma, twenty]));
        assert!(!dfa.accepts(&[peters, comma, andy, comma, twenty]));
        assert!(!dfa.accepts(&[andy, comma, peters, comma, twenty]));
    };

    let test_dfa2 = |dfa2: &Dfa| {
        assert!(dfa2.accepts(&[peters, comma, twenty]));
        assert!(dfa2.accepts(&[andy, peters, comma, twenty]));
        assert!(!dfa2.accepts(&[peters, comma, reuben, comma, twenty]));
        assert!(!dfa2.accepts(&[reuben, peters, comma, twenty]));
    };

    test_dfa(&dfa);
    test_dfa(&dfa_brz);
    test_dfa2(&dfa2);
    test_dfa2(&dfa2_brz);
}

#[test]
fn test_brzozowski_minimise() {
    let a = Edge(1);
    let b = Edge(2);
    let c = Edge(3);
    let d = Edge(4);
    let e = Edge(5);
    let nfa = {
        let mut nfa = Nfa::new();
        nfa.add_complete_sequence(vec![a, b, c, e]);
        nfa.add_complete_sequence(vec![a, b, e]);
        nfa.add_complete_sequence(vec![b, c, d, e]);
        nfa.add_complete_sequence(vec![b, d, e]);
        nfa
    };

    let dfa = nfa.brzozowski_minimise();
    println!("abcde {:?}", Dot::with_config(&dfa.graph, &[]));

    assert!(dfa.accepts(&[a, b, e]));
    assert!(!dfa.accepts(&[a, b, c, d, e]));
}

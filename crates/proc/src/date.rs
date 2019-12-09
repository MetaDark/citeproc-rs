// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Copyright © 2018 Corporation for Digital Scholarship

use crate::prelude::*;

use citeproc_io::Date;
use csl::terms::*;
use csl::Atom;
use csl::LocaleDate;
use csl::{
    BodyDate, DatePart, DatePartForm, DateParts, DayForm, IndependentDate, Locale, LocalizedDate,
    MonthForm, SortKey, YearForm,
};
use std::fmt::Write;
use std::mem;

enum Either<O: OutputFormat> {
    Build(Option<O::Build>),
    /// We will convert this to RefIR as necessary. It will only contain Outputs and
    /// YearSuffixHooks It will only contain Outputs and YearSuffixHooks.
    /// It will not be Rendered(None).
    Ir(IR<O>),
}

impl<O: OutputFormat> Either<O> {
    fn into_cite_ir(self) -> IrSum<O> {
        match self {
            Either::Build(opt) => {
                let content = opt.map(CiteEdgeData::Output);
                let gv = GroupVars::rendered_if(content.is_some());
                (IR::Rendered(content), gv)
            }
            Either::Ir(ir) => {
                let gv = if let IR::Rendered(None) = &ir {
                    GroupVars::OnlyEmpty
                } else {
                    GroupVars::DidRender
                };
                (ir, gv)
            }
        }
    }
}

fn to_ref_ir(
    ir: IR<Markup>,
    stack: Formatting,
    ys_edge: Edge,
    to_edge: &impl Fn(Option<CiteEdgeData<Markup>>, Formatting) -> Option<Edge>,
) -> RefIR {
    match ir {
        // Either Rendered(Some(CiteEdgeData::YearSuffix)) or explicit year suffixes can end up as
        // EdgeData::YearSuffix edges in RefIR. Because we don't care whether it's been rendered or
        // not -- in RefIR's comparison, it must always be an EdgeData::YearSuffix.
        IR::Rendered(opt_build) => RefIR::Edge(to_edge(opt_build, stack)),
        IR::YearSuffix(_ysh, _opt_build) => RefIR::Edge(Some(ys_edge)),
        IR::Seq(ir_seq) => RefIR::Seq(RefIrSeq {
            contents: ir_seq
                .contents
                .into_iter()
                .map(|x| to_ref_ir(x, stack, ys_edge, to_edge))
                .collect(),
            formatting: ir_seq.formatting,
            affixes: ir_seq.affixes,
            delimiter: ir_seq.delimiter,
        }),
        IR::ConditionalDisamb(..) | IR::Name(_) => unreachable!(),
    }
}

impl Either<Markup> {
    fn into_ref_ir(
        self,
        db: &impl IrDatabase,
        ctx: &RefContext<Markup>,
        stack: Formatting,
    ) -> (RefIR, GroupVars) {
        let fmt = ctx.format;
        let to_edge =
            |opt_cite_edge: Option<CiteEdgeData<Markup>>, stack: Formatting| -> Option<Edge> {
                opt_cite_edge.map(|cite_edge| db.edge(cite_edge.to_edge_data(fmt, stack)))
            };
        let ys_edge = db.edge(EdgeData::YearSuffix);
        match self {
            Either::Build(opt) => {
                let content = opt.map(CiteEdgeData::Output);
                let edge = to_edge(content, stack);
                let gv = GroupVars::rendered_if(edge.is_some());
                (RefIR::Edge(edge), gv)
            }
            Either::Ir(ir) => {
                // If it's Ir we'll assume there is a year suffix hook in there -- so not
                // Rendered(None), at least.
                (
                    to_ref_ir(ir, stack, ys_edge, &to_edge),
                    GroupVars::DidRender,
                )
            }
        }
    }
}

impl<'c, O, I> Proc<'c, O, I> for BodyDate
where
    O: OutputFormat,
    I: OutputFormat,
{
    fn intermediate(
        &self,
        _db: &impl IrDatabase,
        _state: &mut IrState,
        ctx: &CiteContext<'c, O, I>,
    ) -> IrSum<O> {
        match self {
            BodyDate::Indep(idate) => intermediate_generic_indep(idate, GenericContext::Cit(ctx)),
            BodyDate::Local(ldate) => intermediate_generic_local(ldate, GenericContext::Cit(ctx)),
        }
        .map(Either::into_cite_ir)
        .unwrap_or((IR::Rendered(None), GroupVars::rendered_if(false)))
    }
}

impl Disambiguation<Markup> for BodyDate {
    fn ref_ir(
        &self,
        db: &impl IrDatabase,
        ctx: &RefContext<Markup>,
        _state: &mut IrState,
        stack: Formatting,
    ) -> (RefIR, GroupVars) {
        let _fmt = ctx.format;
        match self {
            BodyDate::Indep(idate) => {
                intermediate_generic_indep::<Markup, Markup>(idate, GenericContext::Ref(ctx))
            }
            BodyDate::Local(ldate) => {
                intermediate_generic_local::<Markup, Markup>(ldate, GenericContext::Ref(ctx))
            }
        }
        .map(|e| e.into_ref_ir(db, ctx, stack))
        .unwrap_or((RefIR::Edge(None), GroupVars::rendered_if(false)))
    }
}

struct GenericDateBits<'a> {
    overall_formatting: Option<Formatting>,
    overall_affixes: &'a Affixes,
    overall_delimiter: &'a Atom,
    display: Option<DisplayMode>,
}

struct PartBuilder<'a, O: OutputFormat> {
    bits: GenericDateBits<'a>,
    acc: PartAccumulator<O>,
}

enum PartAccumulator<O: OutputFormat> {
    Builds(Vec<O::Build>),
    Seq(IrSeq<O>),
}

impl<'a, O: OutputFormat> PartBuilder<'a, O> {
    fn new(bits: GenericDateBits<'a>, len_hint: usize) -> Self {
        PartBuilder {
            bits,
            acc: PartAccumulator::Builds(Vec::with_capacity(len_hint)),
        }
    }

    fn upgrade(&mut self) {
        let PartBuilder {
            ref mut acc,
            ref mut bits,
        } = self;
        *acc = match acc {
            PartAccumulator::Builds(ref mut vec) => {
                let vec = mem::replace(vec, Vec::new());
                let mut seq = IrSeq {
                    contents: Vec::with_capacity(vec.capacity()),
                    formatting: bits.overall_formatting,
                    delimiter: bits.overall_delimiter.clone(),
                    affixes: bits.overall_affixes.clone(),
                    display: bits.display,
                };
                for built in vec {
                    seq.contents
                        .push(IR::Rendered(Some(CiteEdgeData::Output(built))))
                }
                PartAccumulator::Seq(seq)
            }
            _ => return,
        }
    }

    fn push_either(&mut self, either: Either<O>) {
        match either {
            Either::Ir(ir) => {
                self.upgrade();
                match &mut self.acc {
                    PartAccumulator::Seq(ref mut seq) => {
                        seq.contents.push(ir);
                    }
                    _ => unreachable!(),
                }
            }
            Either::Build(Some(built)) => match &mut self.acc {
                PartAccumulator::Builds(ref mut vec) => {
                    vec.push(built);
                }
                PartAccumulator::Seq(ref mut seq) => seq
                    .contents
                    .push(IR::Rendered(Some(CiteEdgeData::Output(built)))),
            },
            Either::Build(None) => {}
        }
    }

    pub fn into_either(self, fmt: &O) -> Either<O> {
        let PartBuilder { bits, acc } = self;
        match acc {
            PartAccumulator::Builds(each) => {
                if each.is_empty() {
                    return Either::Build(None);
                }
                let built = fmt.affixed(
                    fmt.group(each, &bits.overall_delimiter, bits.overall_formatting),
                    bits.overall_affixes,
                );
                Either::Build(Some(built))
            }
            PartAccumulator::Seq(seq) => Either::Ir(IR::Seq(seq)),
        }
    }
}

fn intermediate_generic_local<'c, O, I>(
    local: &LocalizedDate,
    ctx: GenericContext<'c, O, I>,
) -> Option<Either<O>>
where
    O: OutputFormat,
    I: OutputFormat,
{
    let fmt = ctx.format();
    let locale = ctx.locale();
    let refr = ctx.reference();
    // TODO: handle missing
    let locale_date: &LocaleDate = locale.dates.get(&local.form).unwrap();
    let empty = GenericDateBits {
        overall_delimiter: &Atom::from(""),
        overall_formatting: None,
        overall_affixes: &crate::sort::natural_sort::date_affixes(),
        display: None,
    };
    let gen_date = if ctx.sort_key().is_some() {
        empty
    } else {
        GenericDateBits {
            overall_delimiter: &locale_date.delimiter.0,
            overall_formatting: local.formatting,
            overall_affixes: &local.affixes,
            display: if ctx.in_bibliography() {
                local.display
            } else {
                None
            },
        }
    };
    // TODO: render date ranges
    // TODO: TextCase
    let date = refr
        .date
        .get(&local.variable)
        .and_then(|r| r.single_or_first());
    date.map(|val| {
        let len_hint = locale_date.date_parts.len();
        let rendered_parts = locale_date
            .date_parts
            .iter()
            .filter(|dp| dp_matches(dp, local.parts_selector))
            .filter_map(|dp| dp_render_either(dp, ctx.clone(), &val));
        let mut builder = PartBuilder::new(gen_date, len_hint);
        for (_form, either) in rendered_parts {
            builder.push_either(either);
        }
        builder.into_either(fmt)
    })
}

fn intermediate_generic_indep<'c, O, I>(
    indep: &IndependentDate,
    ctx: GenericContext<'c, O, I>,
) -> Option<Either<O>>
where
    O: OutputFormat,
    I: OutputFormat,
{
    let fmt = ctx.format();
    let empty = GenericDateBits {
        overall_delimiter: &Atom::from(""),
        overall_formatting: None,
        overall_affixes: &crate::sort::natural_sort::date_affixes(),
        display: None,
    };
    let gen_date = if ctx.sort_key().is_some() {
        empty
    } else {
        GenericDateBits {
            overall_delimiter: &indep.delimiter.0,
            overall_formatting: indep.formatting,
            overall_affixes: &indep.affixes,
            display: if ctx.in_bibliography() {
                indep.display
            } else {
                None
            },
        }
    };
    let date = ctx
        .reference()
        .date
        .get(&indep.variable)
        // TODO: render date ranges
        .and_then(|r| r.single_or_first());
    date.map(|val| {
        let len_hint = indep.date_parts.len();
        let each = indep
            .date_parts
            .iter()
            .filter_map(|dp| dp_render_either(dp, ctx.clone(), &val));
        let mut builder = PartBuilder::new(gen_date, len_hint);
        for (_form, either) in each {
            builder.push_either(either);
        }
        builder.into_either(fmt)
    })
}

fn dp_matches(part: &DatePart, selector: DateParts) -> bool {
    match part.form {
        DatePartForm::Day(_) => selector == DateParts::YearMonthDay,
        DatePartForm::Month(..) => selector != DateParts::Year,
        DatePartForm::Year(_) => true,
    }
}

fn dp_render_either<'c, O: OutputFormat, I: OutputFormat>(
    part: &DatePart,
    ctx: GenericContext<'c, O, I>,
    date: &Date,
) -> Option<(DatePartForm, Either<O>)> {
    let fmt = ctx.format();
    if let Some(key) = ctx.sort_key() {
        let string = dp_render_sort_string(part, date, key);
        return string.map(|s| (part.form, Either::Build(Some(fmt.text_node(s, None)))));
    }
    let string = dp_render_string(part, &ctx, date);
    string
        .map(|s| {
            if let DatePartForm::Year(_) = part.form {
                Either::Ir({
                    let year_part = IR::Rendered(Some(CiteEdgeData::Output(fmt.plain(&s))));
                    let mut contents = Vec::with_capacity(2);
                    contents.push(year_part);
                    if ctx.should_add_year_suffix_hook() {
                        let hook = IR::YearSuffix(YearSuffixHook::Plain, None);
                        contents.push(hook);
                    }
                    IR::Seq(IrSeq {
                        contents,
                        affixes: part.affixes.clone(),
                        formatting: part.formatting,
                        delimiter: Atom::from(""),
                        display: None,
                    })
                })
            } else {
                Either::Build(Some(fmt.affixed_text(s, part.formatting, &part.affixes)))
            }
        })
        .map(|x| (part.form, x))
}

fn dp_render_sort_string(part: &DatePart, date: &Date, key: &SortKey) -> Option<String> {
    let should_return_zeroes = key.is_macro();
    match part.form {
        DatePartForm::Year(_form) => Some(format!("{:04}", date.year)),
        DatePartForm::Month(_form, _strip_periods) => {
            // Sort strings do not compare seasons
            if date.month > 0 && date.month <= 12 {
                Some(format!("{:02}", date.month))
            } else if date.month == 0 && should_return_zeroes {
                Some("00".to_owned())
            } else {
                None
            }
        }
        DatePartForm::Day(_form) => {
            if date.day == 0 && should_return_zeroes {
                None
            } else {
                Some(format!("{:02}", date.day))
            }
        }
    }
}

fn render_year(year: i32, form: YearForm, locale: &Locale) -> String {
    let mut s = String::new();
    // Only do short form ('07) for four-digit years
    match (form, year > 1000) {
        (YearForm::Short, true) => write!(s, "{:02}", year.abs() % 100).unwrap(),
        (YearForm::Long, _) | (YearForm::Short, false) => write!(s, "{}", year.abs()).unwrap(),
    }
    if year < 0 {
        let sel = SimpleTermSelector::Misc(MiscTerm::Bc, TermFormExtended::Long);
        let sel = TextTermSelector::Simple(sel);
        if let Some(bc) = locale.get_text_term(sel, false) {
            s.push_str(bc);
        } else {
            s.push_str("BC");
        }
    } else if year < 1000 {
        let sel = SimpleTermSelector::Misc(MiscTerm::Ad, TermFormExtended::Long);
        let sel = TextTermSelector::Simple(sel);
        if let Some(ad) = locale.get_text_term(sel, false) {
            s.push_str(ad);
        } else {
            s.push_str("AD");
        }
    }
    s
}

fn dp_render_string<'c, O: OutputFormat, I: OutputFormat>(
    part: &DatePart,
    ctx: &GenericContext<'c, O, I>,
    date: &Date,
) -> Option<String> {
    let locale = ctx.locale();
    match part.form {
        DatePartForm::Year(form) => Some(render_year(date.year, form, ctx.locale())),
        DatePartForm::Month(form, _strip_periods) => match form {
            MonthForm::Numeric => {
                if date.month == 0 || date.month > 12 {
                    None
                } else {
                    Some(format!("{}", date.month))
                }
            }
            MonthForm::NumericLeadingZeros => {
                if date.month == 0 || date.month > 12 {
                    None
                } else {
                    Some(format!("{:02}", date.month))
                }
            }
            _ => {
                let sel = GenderedTermSelector::from_month_u32(date.month, form)?;
                Some(
                    locale
                        .gendered_terms
                        .get(&sel)
                        .map(|gt| gt.0.singular().to_string())
                        .unwrap_or_else(|| {
                            let fallback = if form == MonthForm::Short {
                                MONTHS_SHORT
                            } else {
                                MONTHS_LONG
                            };
                            fallback[date.month as usize].to_string()
                        }),
                )
            }
        },
        DatePartForm::Day(form) => match form {
            DayForm::Numeric => {
                if date.day == 0 {
                    None
                } else {
                    Some(format!("{}", date.day))
                }
            }
            DayForm::NumericLeadingZeros => {
                if date.day == 0 {
                    None
                } else {
                    Some(format!("{:02}", date.day))
                }
            }
            // TODO: implement ordinals
            DayForm::Ordinal => {
                if date.day == 0 {
                    None
                } else {
                    Some(format!("{}ORD", date.day))
                }
            }
        },
    }
}

// Some fallbacks so we don't have to panic so much if en-US is absent.

const MONTHS_SHORT: &[&str] = &[
    "undefined",
    "Jan",
    "Feb",
    "Mar",
    "Apr",
    "May",
    "Jun",
    "Jul",
    "Aug",
    "Sep",
    "Oct",
    "Nov",
    "Dec",
    "Spring",
    "Summer",
    "Autumn",
    "Winter",
];

const MONTHS_LONG: &[&str] = &[
    "undefined",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
    "Spring",
    "Summer",
    "Autumn",
    "Winter",
];

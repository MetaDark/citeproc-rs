use codespan::{CodeMap, FileMap, Span};
use codespan_reporting::termcolor::{ColorChoice, StandardStream};
use codespan_reporting::{emit, Diagnostic, Label, Severity};
use roxmltree::{Error, Node, TextPos};
use std::num::ParseIntError;

#[derive(Debug, PartialEq)]
pub struct UnknownAttributeValue {
    pub value: String,
}

impl UnknownAttributeValue {
    pub fn new(s: &str) -> Self {
        UnknownAttributeValue {
            value: s.to_owned(),
        }
    }
}

#[derive(Debug)]
pub enum StyleError {
    ValidationError(InvalidCsl),
    ParseError(Error),
}

#[derive(Debug, PartialEq)]
pub struct InvalidCsl {
    pub severity: Severity,
    pub text_pos: TextPos,
    pub len: usize,
    pub message: String,
}

impl InvalidCsl {
    pub fn new(node: &Node, message: String) -> Self {
        let mut pos = node.node_pos();
        pos.col += 1;
        InvalidCsl {
            text_pos: pos,
            len: node.tag_name().name().len(),
            severity: Severity::Error,
            message,
        }
    }

    pub fn bad_int(node: &Node, attr: &str, uav: &ParseIntError) -> Self {
        InvalidCsl {
            text_pos: node
                .attribute_value_pos(attr)
                .unwrap_or_else(|| node.node_pos()),
            len: node.attribute("attr").map(|a| a.len()).unwrap_or_else(|| 1),
            message: format!("Invalid integer value for {}: {:?}", attr, uav),
            severity: Severity::Error,
        }
    }

    pub fn attr_val(node: &Node, attr: &str, uav: &str) -> Self {
        InvalidCsl {
            text_pos: node
                .attribute_value_pos(attr)
                .unwrap_or_else(|| node.node_pos()),
            len: uav.len(),
            message: format!("Unknown attribute value for {}: \"{}\"", attr, uav),
            severity: Severity::Error,
        }
    }

    pub fn to_diagnostic(&self, file_map: &FileMap) -> Option<Diagnostic> {
        let str_start = file_map
            .byte_index(
                (self.text_pos.row - 1 as u32).into(),
                (self.text_pos.col - 1 as u32).into(),
            )
            .ok()?;

        Some(
            Diagnostic::new(self.severity, self.message.clone())
            .with_label(
                Label::new_primary(Span::from_offset(str_start, (self.len as i64).into()))
                .with_message("")
                // .with_message(self.message.clone()),
            ),
        )
    }
}

impl From<Error> for StyleError {
    fn from(err: Error) -> StyleError {
        StyleError::ParseError(err)
    }
}

impl From<InvalidCsl> for StyleError {
    fn from(err: InvalidCsl) -> StyleError {
        StyleError::ValidationError(err)
    }
}

fn get_pos(e: &Error) -> TextPos {
    match *e {
        Error::InvalidXmlPrefixUri(pos) => pos,
        Error::UnexpectedXmlUri(pos) => pos,
        Error::UnexpectedXmlnsUri(pos) => pos,
        Error::InvalidElementNamePrefix(pos) => pos,
        Error::DuplicatedNamespace(ref _name, pos) => pos,
        Error::UnexpectedCloseTag { pos, .. } => pos,
        Error::UnexpectedEntityCloseTag(pos) => pos,
        Error::UnknownEntityReference(ref _name, pos) => pos,
        Error::EntityReferenceLoop(pos) => pos,
        Error::DuplicatedAttribute(ref _name, pos) => pos,
        _ => TextPos::new(0, 0),
    }
}

impl StyleError {
    pub fn to_diagnostic(&self, file_map: &FileMap) -> Option<Diagnostic> {
        match *self {
            StyleError::ValidationError(ref e) => e.to_diagnostic(file_map),
            StyleError::ParseError(ref e) => {
                let pos = get_pos(&e);

                let str_start = file_map
                    .byte_index((pos.row - 1 as u32).into(), (pos.col - 1 as u32).into())
                    .ok()?;

                Some(
                    Diagnostic::new(Severity::Error, format!("{}", e)).with_label(
                        Label::new_primary(Span::from_offset(str_start, (0 as i64).into()))
                            .with_message(""),
                    ),
                )
            }
        }
    }
}

impl Default for StyleError {
    fn default() -> Self {
        StyleError::ValidationError(InvalidCsl {
            severity: Severity::Error,
            text_pos: TextPos::new(0, 0),
            len: 0,
            message: String::from(""),
        })
    }
}

pub fn file_diagnostics<'a>(diagnostics: &[StyleError], filename: &'a str, document: &'a str) {
    let mut code_map = CodeMap::new();
    let file_map = code_map.add_filemap(filename.to_owned().into(), document.to_string());
    let writer = StandardStream::stderr(ColorChoice::Auto);
    for diag in diagnostics.iter().map(|d| d.to_diagnostic(&file_map)) {
        if let Some(d) = diag {
            emit(&mut writer.lock(), &code_map, &d).unwrap();
            println!();
        }
    }
}
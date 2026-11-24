//! A Fluent catalog's messages and the variables each takes, read from its source. The build reads the English
//! catalog with it to generate [`crate::Phrase`] and the typed methods on [`crate::Words`]; a test reads every other
//! catalog with it and compares.

use fluent_syntax::ast::{self, Entry, Expression, InlineExpression, PatternElement};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// One message: its id, its comment's lines, its value as written on one line, its value as a window shows it where
/// that is only text, and every variable it or a message it names reads.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub id: String,
    pub comment: Vec<String>,
    pub text: String,
    pub plain: Option<String>,
    pub variables: BTreeSet<String>,
}

/// How the English catalog marks a message without a value as text, never a control's name: its comment begins so.
pub(crate) const TEXT_MARK: &str = "Text:";

/// What a message is, as the English catalog writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Use {
    /// It names a value: a method on `Words`.
    Values,
    /// It names none and may name a control: a `Phrase`, held to a label's bound in every catalog.
    Label,
    /// It names none and its comment begins with [`TEXT_MARK`]: a `Text`, shown as text and as long as it needs.
    Text,
}

impl Message {
    /// What this message is, read from the English catalog.
    pub(crate) fn used(&self) -> Use {
        if !self.variables.is_empty() {
            Use::Values
        } else if self.comment.first().is_some_and(|line| line.starts_with(TEXT_MARK)) {
            Use::Text
        } else {
            Use::Label
        }
    }
}

/// Why a catalog cannot be read.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    Syntax {
        line: usize,
    },
    Twice {
        id: String,
    },
    /// An attribute, or a message with no value: neither has a place in what a window says.
    NotPlain {
        id: String,
    },
    UnknownMessage {
        id: String,
        named: String,
    },
    /// A message the English catalog uses as a label is empty or longer than a label holds.
    NotLabel {
        id: String,
        why: design::LabelError,
    },
    /// A message the English catalog uses as a label is written with a placeable, so its length is not its text's.
    LabelNotPlain {
        id: String,
    },
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Syntax { line } => write!(f, "line {line}: not Fluent"),
            Fault::Twice { id } => write!(f, "`{id}` is defined twice"),
            Fault::NotPlain { id } => write!(f, "`{id}` has an attribute or no value"),
            Fault::UnknownMessage { id, named } => write!(f, "`{id}` names `{named}`, which is not defined"),
            Fault::NotLabel { id, why } => {
                write!(f, "`{id}` is a label, and {why}: shorten it, or mark it `{TEXT_MARK}` in English if it is text")
            }
            Fault::LabelNotPlain { id } => write!(f, "`{id}` is a label, and a label is plain text"),
        }
    }
}

/// The messages of `source`, in order.
pub(crate) fn read(source: &str) -> Result<Vec<Message>, Fault> {
    let resource = fluent_syntax::parser::parse(source).map_err(|(_, errors)| {
        let at = errors.first().map_or(0, |error| error.pos.start);
        Fault::Syntax { line: source.bytes().take(at).filter(|&byte| byte == b'\n').count() + 1 }
    })?;
    let mut read: Vec<(Message, BTreeSet<String>)> = Vec::new();
    for entry in &resource.body {
        let Entry::Message(message) = entry else { continue };
        let id = message.id.name.to_string();
        if read.iter().any(|(known, _)| known.id == id) {
            return Err(Fault::Twice { id });
        }
        let Some(value) = message.value.as_ref().filter(|_| message.attributes.is_empty()) else {
            return Err(Fault::NotPlain { id });
        };
        let mut found = Found::default();
        found.pattern(value);
        let comment = message.comment.iter().flat_map(|c| c.content.iter().map(|line| line.to_string())).collect();
        let text = written(message);
        let plain = plain(value);
        read.push((Message { id, comment, text, plain, variables: found.variables }, found.messages));
    }
    let direct: BTreeMap<&str, (&BTreeSet<String>, &BTreeSet<String>)> =
        read.iter().map(|(message, named)| (message.id.as_str(), (&message.variables, named))).collect();
    let mut whole = Vec::with_capacity(read.len());
    for (message, _) in &read {
        let variables = reached(&message.id, &direct)?;
        whole.push(Message {
            id: message.id.clone(),
            comment: message.comment.clone(),
            text: message.text.clone(),
            plain: message.plain.clone(),
            variables,
        });
    }
    Ok(whole)
}

/// Refuses `catalog` where a message `english` uses as a label is not one: written with a placeable, or, as a window
/// shows it, empty or longer than a label holds.
pub(crate) fn labels(english: &[Message], catalog: &[Message]) -> Result<(), Fault> {
    let labels: BTreeSet<&str> =
        english.iter().filter(|message| message.used() == Use::Label).map(|message| message.id.as_str()).collect();
    for message in catalog.iter().filter(|message| labels.contains(message.id.as_str())) {
        let Some(plain) = &message.plain else { return Err(Fault::LabelNotPlain { id: message.id.clone() }) };
        design::Label::check(plain).map_err(|why| Fault::NotLabel { id: message.id.clone(), why })?;
    }
    Ok(())
}

/// A value of text alone as Fluent formats it, its lines joined as written; none where it has a placeable.
fn plain(value: &ast::Pattern<&str>) -> Option<String> {
    value
        .elements
        .iter()
        .map(|element| match element {
            PatternElement::TextElement { value } => Some(*value),
            PatternElement::Placeable { .. } => None,
        })
        .collect()
}

/// A message's value as Fluent writes it, its lines joined by a space.
fn written(message: &ast::Message<&str>) -> String {
    let bare = ast::Message { comment: None, attributes: Vec::new(), ..message.clone() };
    let source = fluent_syntax::serializer::serialize(&ast::Resource { body: vec![Entry::Message(bare)] });
    let value = source.split_once('=').map_or(source.as_str(), |(_, value)| value);
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The variables `id` reads, through every message it names (a named message shares its caller's values).
fn reached(
    id: &str,
    direct: &BTreeMap<&str, (&BTreeSet<String>, &BTreeSet<String>)>,
) -> Result<BTreeSet<String>, Fault> {
    let mut variables = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut next = vec![id.to_string()];
    while let Some(at) = next.pop() {
        if !seen.insert(at.clone()) {
            continue;
        }
        let Some((own, named)) = direct.get(at.as_str()) else {
            return Err(Fault::UnknownMessage { id: id.to_string(), named: at });
        };
        variables.extend(own.iter().cloned());
        next.extend(named.iter().cloned());
    }
    Ok(variables)
}

#[derive(Default)]
struct Found {
    variables: BTreeSet<String>,
    messages: BTreeSet<String>,
}

impl Found {
    fn pattern(&mut self, pattern: &ast::Pattern<&str>) {
        for element in &pattern.elements {
            if let PatternElement::Placeable { expression } = element {
                self.expression(expression);
            }
        }
    }

    fn expression(&mut self, expression: &Expression<&str>) {
        match expression {
            Expression::Select { selector, variants } => {
                self.inline(selector);
                for variant in variants {
                    self.pattern(&variant.value);
                }
            }
            Expression::Inline(inline) => self.inline(inline),
        }
    }

    /// A term's own variables are its arguments, not the message's, so only the values passed to it are read.
    fn inline(&mut self, inline: &InlineExpression<&str>) {
        match inline {
            InlineExpression::VariableReference { id } => {
                self.variables.insert(id.name.to_string());
            }
            InlineExpression::MessageReference { id, .. } => {
                self.messages.insert(id.name.to_string());
            }
            InlineExpression::FunctionReference { arguments, .. } => self.arguments(arguments),
            InlineExpression::TermReference { arguments, .. } => {
                if let Some(arguments) = arguments {
                    self.arguments(arguments);
                }
            }
            InlineExpression::Placeable { expression } => self.expression(expression),
            InlineExpression::StringLiteral { .. } | InlineExpression::NumberLiteral { .. } => {}
        }
    }

    fn arguments(&mut self, arguments: &ast::CallArguments<&str>) {
        for value in arguments.positional.iter().chain(arguments.named.iter().map(|named| &named.value)) {
            self.inline(value);
        }
    }
}

/// How a translation differs from the catalog it translates.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Mismatch {
    Missing { id: String },
    Extra { id: String },
    Variables { id: String, wanted: BTreeSet<String>, found: BTreeSet<String> },
}

/// Every way `other` differs from `reference`: the same ids, each reading the same variables.
pub(crate) fn differences(reference: &[Message], other: &[Message]) -> Vec<Mismatch> {
    let theirs: BTreeMap<&str, &Message> = other.iter().map(|message| (message.id.as_str(), message)).collect();
    let ours: BTreeSet<&str> = reference.iter().map(|message| message.id.as_str()).collect();
    let mut found = Vec::new();
    for message in reference {
        match theirs.get(message.id.as_str()) {
            None => found.push(Mismatch::Missing { id: message.id.clone() }),
            Some(translated) if translated.variables != message.variables => found.push(Mismatch::Variables {
                id: message.id.clone(),
                wanted: message.variables.clone(),
                found: translated.variables.clone(),
            }),
            Some(_) => {}
        }
    }
    found.extend(
        other
            .iter()
            .filter(|message| !ours.contains(message.id.as_str()))
            .map(|message| Mismatch::Extra { id: message.id.clone() }),
    );
    found
}

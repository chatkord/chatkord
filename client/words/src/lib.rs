//! What the window says, apart from what decides it: each carried language's catalog, the sentences a value goes
//! into, how a measured number reads, and what each failure says. A screen chooses the value; this crate words it.
#![forbid(unsafe_code)]

#[cfg(test)]
#[expect(dead_code, reason = "the tests compare catalogs; their comments are the build's")]
mod catalog;
mod choices;
pub mod failure;
mod figure;
mod sentence;
mod tag;
mod words;

pub use failure::Failure;
pub use sentence::{Ends, Sentence};
pub use unic_langid::LanguageIdentifier;
pub use words::without_isolation;
pub use words::{Language, Phrase, Text, Words};

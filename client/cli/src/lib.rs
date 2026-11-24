//! The command line: register, sign in, list and join channels, speak, listen, share and watch. The session token
//! is kept in the keyring and a password read once without echo; it asks the server and says the server's sentence.
#![forbid(unsafe_code)]

mod command;
mod console;
mod record;
mod run;
mod spool;

pub use command::{Access, Account, Act, Command, InviteTerms, WatchTo, Whom, parse};
pub use console::main;
pub use run::{Ask, Terminal, run};

/// What a failure of the link says to a person, in the window's words; a key to trust is named, for the window to
/// trust it, as the command line asks no one to.
fn said(failed: &terms::LinkError) -> String {
    let said = english(&words::failure::said(failed));
    if let terms::LinkError::Unvouched(pin) = failed {
        return format!("{said} Its fingerprint is {pin}; trust it in the Chatkord window.");
    }
    said
}

/// What a failure of the call, its devices or its screen says.
fn failed(failed: &words::Failure) -> String {
    english(&words::failure::of(failed))
}

/// The same for a file that was not sent or fetched.
fn file_said(failed: &terms::FileError) -> String {
    english(&words::failure::file(failed))
}

/// What `sentence` says. The command line speaks English only, and says a sentence once, so it reads the catalog
/// where it speaks.
fn english(sentence: &words::Sentence) -> String {
    words::Words::english().said(sentence)
}

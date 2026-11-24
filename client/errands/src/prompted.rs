//! A sign-in at an address already named, asking a person for the password only when the saved token is not enough:
//! the sign-in model's steps, each errand it names carried by the door's own carrier.

use crate::Session;
use crate::door::carry_out;
use crate::signin::named_identity;
use link::Trust;
use screens::{Effect, Input, SignIn, Target};
use std::collections::VecDeque;
use std::future::Future;
use terms::{LinkError, Secret, ServerUrl};
use words::{Phrase, Sentence};

/// Whom a sign-in at a named address asks for the password and tells what happens: a person at a terminal, or a
/// host that answers from where it can.
pub trait Prompt {
    /// The password, asked only when the saved token is not enough.
    fn password(&mut self) -> impl Future<Output = Result<Secret, String>> + Send;
    /// What the sign-in says along the way.
    fn say(&mut self, said: Phrase);
}

/// Why a sign-in at a named address did not open a session.
#[derive(Debug)]
pub enum Stopped {
    /// What the sign-in says to a person.
    Failed(Sentence),
    /// The prompt gave no password, in its own words.
    Prompt(String),
}

/// Signs in at an address already named as the socket it means, with the saved token if the server still takes it,
/// else with the password `prompt` gives, which comes back with the session for a password change to prove.
pub async fn sign_in_at(
    platform: &platform::Platform,
    url: &ServerUrl,
    identity: &str,
    trust: &Trust,
    build: &str,
    prompt: &mut impl Prompt,
) -> Result<(Session, Option<Secret>), Stopped> {
    // An address named as the socket it means already greeted: nothing is resolved again here.
    let mut model = SignIn::at(Target { server: url.clone(), identity: identity.to_string() });
    let mut effects: VecDeque<Effect> = model.update(Input::Begin).into();
    let mut opened: Option<Session> = None;
    let mut typed = None;
    while let Some(effect) = effects.pop_front() {
        match effect {
            Effect::Go(errand) => {
                let carried = carry_out(platform, errand, trust, build).await;
                opened = carried.session.or(opened);
                // The sign-in's errands are answered as its own reports, or not at all.
                if let Some(screens::Heard::Flow(report)) = carried.heard {
                    effects.extend(model.update(report.into()));
                }
            }
            Effect::AskPassword => {
                let password = prompt.password().await.map_err(Stopped::Prompt)?;
                typed = Some(Secret::new(password.expose().to_string()));
                effects.extend(model.update(Input::Password(password)));
            }
            Effect::Expired(reason) => {
                prompt.say(reason.and_then(words::failure::closed).unwrap_or(Phrase::SavedSignInExpired));
            }
            Effect::Say(phrase) => prompt.say(phrase),
            Effect::Fail(failed) => return Err(Stopped::Failed(words::failure::said(&failed))),
            Effect::Done => break,
        }
    }
    let session = opened.ok_or_else(|| Stopped::Failed(words::failure::said(&LinkError::Malformed)))?;
    // A kept token stores nothing new, so the account is remembered here too, for the window to find it.
    let filed = platform::keyring::filed_under(identity, named_identity(&session.ack));
    let _ = platform::state::remember_account(platform, url.clone(), filed).await;
    Ok((session, typed))
}

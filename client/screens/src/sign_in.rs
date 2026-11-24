//! Signing in: use the token saved for this server and account if there is one and the server still takes it, else
//! ask for the password; keep the token the server issues, and say so when it cannot be kept.

use crate::door::Errand;
use proto::chatkord_v1::{CloseReason, ErrorCode};
use terms::MemberId;
use terms::{LinkError, Secret, ServerUrl};
use words::Phrase;

/// Where a sign-in goes: the address that greeted and the username.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The address that greeted, the route the credential goes by.
    pub server: ServerUrl,
    /// The username the account signs in with, trimmed.
    pub identity: String,
}

/// What the server said of an account as it let it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    /// The account's own id, as the server names a person in a voice room.
    pub user_id: MemberId,
    /// Its username, as typed when it was made.
    pub username: String,
    /// The name others see, as the server said it (its username when none is set); none from a server that did not say.
    pub shown: Option<String>,
    /// Where it stands in the roles' order, as the server places it.
    pub place: proto::rules::Place,
    /// It signed in with a temporary password and may only change it.
    pub must_change: bool,
    /// What its roles let it do.
    pub grants: proto::rules::Grants,
    /// The colour it picked for others to see it in; none for none, or from a server that keeps no colour.
    pub colour: Option<proto::rules::Colour>,
}

/// A refusal, by its code: `Unspecified` for one this build does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// Why it was refused.
    pub code: ErrorCode,
}

impl From<Refusal> for LinkError {
    fn from(refusal: Refusal) -> LinkError {
        LinkError::Refused(refusal.code)
    }
}

/// Where a sign-in is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Idle,
    /// Reading the saved token.
    Loading,
    /// Offering the saved token to the server.
    Trying,
    /// The server did not answer while the saved token was offered: the person is offered that server again, not
    /// asked for a password.
    Retrying,
    AskingPassword,
    /// Offering the password to the server.
    Submitting,
    /// Keeping the token the server issued.
    Storing,
    Done,
}

/// What happened out in the world, as the host tells it: one place says what each is, and the model and the
/// screen both take it.
#[derive(Clone, Debug)]
pub enum Report {
    /// The saved token, or none when there is none or the keyring cannot be used.
    TokenLoaded(Option<Secret>),
    /// The server took the sign-in.
    Accepted {
        /// The token it issued; none when it issued none and the saved one stays.
        token: Option<Secret>,
        /// The identity the store made for the account, when the server named one: where the token is filed.
        named: Option<String>,
        /// What the server said of the account as it let it in.
        standing: Standing,
    },
    /// The server said the saved token no longer signs in, and how its session ended when it said.
    Expired(Option<CloseReason>),
    /// The server answered and said no.
    Refused(Refusal),
    /// The server could not be reached, or ended the connection: nobody's fault but its own.
    Unreached(LinkError),
    /// Whether the issued token was kept.
    Stored(bool),
}

impl From<Report> for Input {
    fn from(report: Report) -> Input {
        match report {
            Report::TokenLoaded(token) => Input::TokenLoaded(token),
            Report::Accepted { token, named, .. } => Input::Accepted { token, named },
            Report::Expired(reason) => Input::Expired(reason),
            Report::Refused(refusal) => Input::Refused(refusal),
            Report::Unreached(failed) => Input::Unreached(failed),
            Report::Stored(kept) => Input::Stored(kept),
        }
    }
}

/// What the host tells the model.
#[derive(Debug)]
pub enum Input {
    /// Start: the saved token is read first.
    Begin,
    /// The saved token, or none when there is none or the keyring cannot be used.
    TokenLoaded(Option<Secret>),
    /// The person gave their password.
    Password(Secret),
    /// The server took the sign-in.
    Accepted {
        /// The token it issued; none when it issued none and the saved one stays.
        token: Option<Secret>,
        /// The identity the store made for the account, when the server named one: where the token is filed.
        named: Option<String>,
    },
    /// The server said the saved token no longer signs in, and how its session ended.
    Expired(Option<CloseReason>),
    /// The server answered and said no: to a password, or to the saved token.
    Refused(Refusal),
    /// The server could not be reached, or ended the connection: nobody's fault but its own.
    Unreached(LinkError),
    /// Whether the issued token was kept.
    Stored(bool),
    /// The person asks for that server again with the sign-in this run already holds: nothing
    /// new is typed, read or written.
    Retry,
}

/// What the sign-in asks: an errand for the host to carry out, or something for whoever signs in to answer or read.
#[derive(Debug)]
pub enum Effect {
    /// Carry out this errand at the sign-in's target; answer with its report.
    Go(Errand),
    /// Ask the person for their password; answer with [`Input::Password`].
    AskPassword,
    /// The saved token is gone, and how its session ended when the server said.
    Expired(Option<CloseReason>),
    /// Tell the person this, which is not a fault.
    Say(Phrase),
    /// This attempt ended in failure, and why.
    Fail(LinkError),
    /// Signed in, the issued token kept or said not to be.
    Done,
}

/// Where a token being offered came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// The session this run held before the connection dropped: offered before the keyring is read.
    Held,
    /// The keyring.
    Saved,
}

/// A held token the server turned away, and how its session ended when the server said, kept while the keyring is
/// read for another.
#[derive(Debug)]
struct Refused {
    token: Secret,
    reason: Option<CloseReason>,
}

/// Where a sign-in is, with the token while it is being offered: asking that server again reads no keyring.
#[derive(Debug)]
enum Stage {
    Idle,
    /// Not begun, with the token this run signed in with, which `Begin` offers first.
    Holding(Secret),
    Loading(Option<Refused>),
    Trying(Secret, Source),
    Retrying(Secret, Source),
    AskingPassword,
    Submitting,
    /// Keeping the issued token, which the session holds once it is kept or said not to be.
    Storing(Secret),
    /// Signed in, holding the token it signed in with: the issued one, else the one it offered.
    Done(Option<Secret>),
}

impl Stage {
    fn phase(&self) -> Phase {
        match self {
            Stage::Idle | Stage::Holding(_) => Phase::Idle,
            Stage::Loading(_) => Phase::Loading,
            Stage::Trying(..) => Phase::Trying,
            Stage::Retrying(..) => Phase::Retrying,
            Stage::AskingPassword => Phase::AskingPassword,
            Stage::Submitting => Phase::Submitting,
            Stage::Storing(_) => Phase::Storing,
            Stage::Done(_) => Phase::Done,
        }
    }
}

/// One sign-in at one target, step by step: the host feeds it each [`Input`] and carries out each [`Effect`] it
/// returns, every errand naming that target.
#[derive(Debug)]
pub struct SignIn {
    stage: Stage,
    target: Target,
}

impl SignIn {
    /// A sign-in at an address that already greeted, waiting for `Begin`.
    pub fn at(target: Target) -> Self {
        SignIn { stage: Stage::Idle, target }
    }

    /// A sign-in that offers `token`, the one this run signed in with, on `Begin` before the keyring is read, and
    /// reads the keyring only if the server turns it away.
    pub(crate) fn holding(target: Target, token: Secret) -> Self {
        SignIn { stage: Stage::Holding(token), target }
    }

    /// The token this sign-in ended holding, once signed in: the one the server issued, else the one it took.
    pub(crate) fn held(&self) -> Option<&Secret> {
        match &self.stage {
            Stage::Done(token) => token.as_ref(),
            Stage::Idle
            | Stage::Holding(_)
            | Stage::Loading(_)
            | Stage::Trying(..)
            | Stage::Retrying(..)
            | Stage::AskingPassword
            | Stage::Submitting
            | Stage::Storing(_) => None,
        }
    }

    /// A sign-in whose credential is already on its way to the server (an account being made): it waits for the
    /// server's answer.
    pub(crate) fn submitting(target: Target) -> Self {
        SignIn { stage: Stage::Submitting, target }
    }

    /// A sign-in that offers `password` now, with the errand that offers it.
    pub(crate) fn with_password(target: Target, password: Secret) -> (Self, Errand) {
        let errand = Errand::SubmitPassword(target.clone(), password);
        (SignIn::submitting(target), errand)
    }

    /// Where it goes.
    pub fn target(&self) -> &Target {
        &self.target
    }

    pub(crate) fn phase(&self) -> Phase {
        self.stage.phase()
    }

    /// Back to waiting for `Begin`, so a late answer to what was under way changes nothing.
    pub(crate) fn stop(&mut self) {
        self.stage = Stage::Idle;
    }

    fn go(&self, errand: impl FnOnce(Target) -> Errand) -> Effect {
        Effect::Go(errand(self.target.clone()))
    }

    /// An input that does not belong to the phase the model is in changes nothing and asks for
    /// nothing, so a late answer cannot start a second sign-in.
    pub fn update(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Begin => match std::mem::replace(&mut self.stage, Stage::Idle) {
                Stage::Holding(token) => {
                    let offer = self.go(|target| Errand::TryToken(target, token.clone()));
                    self.stage = Stage::Trying(token, Source::Held);
                    vec![offer]
                }
                Stage::Idle => {
                    self.stage = Stage::Loading(None);
                    vec![self.go(Errand::LoadToken)]
                }
                stage => self.unchanged(stage),
            },
            Input::TokenLoaded(loaded) => match std::mem::replace(&mut self.stage, Stage::Idle) {
                Stage::Loading(refused) => self.loaded(loaded, refused),
                stage => self.unchanged(stage),
            },
            Input::Password(password) => {
                let offer = self.go(|target| Errand::SubmitPassword(target, password));
                self.on(Phase::AskingPassword, Stage::Submitting, vec![offer])
            }
            Input::Accepted { token, named } => match std::mem::replace(&mut self.stage, Stage::Idle) {
                Stage::Trying(tried, _) => self.admitted(token, Some(tried), named),
                Stage::Submitting => self.admitted(token, None, named),
                stage => self.unchanged(stage),
            },
            Input::Expired(reason) => match std::mem::replace(&mut self.stage, Stage::Idle) {
                // The run's own token is gone, but the keyring may hold a newer one, so it is read before a password
                // is asked for.
                Stage::Trying(token, Source::Held) => {
                    self.stage = Stage::Loading(Some(Refused { token, reason }));
                    vec![self.go(Errand::LoadToken)]
                }
                Stage::Trying(_, Source::Saved) => {
                    self.stage = Stage::AskingPassword;
                    vec![self.go(Errand::ForgetToken), Effect::Expired(reason), Effect::AskPassword]
                }
                stage => self.unchanged(stage),
            },
            Input::Refused(refusal) => match self.phase() {
                Phase::Submitting => {
                    self.stage = Stage::AskingPassword;
                    vec![Effect::Fail(refusal.into())]
                }
                Phase::Trying => {
                    self.stage = Stage::Idle;
                    vec![Effect::Fail(refusal.into())]
                }
                Phase::Idle
                | Phase::Loading
                | Phase::Retrying
                | Phase::AskingPassword
                | Phase::Storing
                | Phase::Done => Vec::new(),
            },
            Input::Unreached(failed) => match std::mem::replace(&mut self.stage, Stage::Idle) {
                // No retry reaches a server of another major, nor one whose key or TLS this device does not take: said
                // once, the saved sign-in kept for an update or a person's word on the key.
                Stage::Trying(..) | Stage::Submitting if failed.kind() == terms::ErrorKind::Lasting => {
                    vec![Effect::Fail(failed)]
                }
                // Nobody answered at the address this run already had: nothing is deleted and no password asked for.
                Stage::Trying(token, source) => {
                    self.stage = Stage::Retrying(token, source);
                    vec![Effect::Say(Phrase::SavedSignInHeld)]
                }
                Stage::Submitting => vec![Effect::Fail(failed)],
                stage => self.unchanged(stage),
            },
            Input::Retry => match std::mem::replace(&mut self.stage, Stage::Idle) {
                Stage::Retrying(token, source) => {
                    let offer = self.go(|target| Errand::TryToken(target, token.clone()));
                    self.stage = Stage::Trying(token, source);
                    vec![offer]
                }
                stage => self.unchanged(stage),
            },
            Input::Stored(kept) => match std::mem::replace(&mut self.stage, Stage::Idle) {
                Stage::Storing(token) => {
                    self.stage = Stage::Done(Some(token));
                    let mut effects = if kept { Vec::new() } else { vec![Effect::Say(Phrase::KeyringUnavailable)] };
                    effects.push(Effect::Done);
                    effects
                }
                stage => self.unchanged(stage),
            },
        }
    }

    /// The keyring answered: its token is offered, unless it is the very one the server just refused, which is
    /// forgotten; with none, the password is asked for, saying why when a held token was refused.
    fn loaded(&mut self, loaded: Option<Secret>, refused: Option<Refused>) -> Vec<Effect> {
        match (loaded, refused) {
            (Some(token), Some(refused)) if token == refused.token => {
                self.stage = Stage::AskingPassword;
                vec![self.go(Errand::ForgetToken), Effect::Expired(refused.reason), Effect::AskPassword]
            }
            (Some(token), _) => {
                let offer = self.go(|target| Errand::TryToken(target, token.clone()));
                self.stage = Stage::Trying(token, Source::Saved);
                vec![offer]
            }
            (None, Some(refused)) => {
                self.stage = Stage::AskingPassword;
                vec![Effect::Expired(refused.reason), Effect::AskPassword]
            }
            (None, None) => {
                self.stage = Stage::AskingPassword;
                vec![Effect::AskPassword]
            }
        }
    }

    /// The server took the sign-in with the password or with `tried`: the issued token is kept, else the sign-in is
    /// done holding the one it offered.
    fn admitted(&mut self, issued: Option<Secret>, tried: Option<Secret>, named: Option<String>) -> Vec<Effect> {
        match issued {
            Some(token) => {
                self.stage = Stage::Storing(token.clone());
                vec![self.go(|target| Errand::StoreToken { target, named, token })]
            }
            None => {
                self.stage = Stage::Done(tried);
                vec![Effect::Done]
            }
        }
    }

    /// An input that did not belong to `stage`, which stays as it was.
    fn unchanged(&mut self, stage: Stage) -> Vec<Effect> {
        self.stage = stage;
        Vec::new()
    }

    fn on(&mut self, from: Phase, to: Stage, effects: Vec<Effect>) -> Vec<Effect> {
        if self.phase() == from {
            self.stage = to;
            effects
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(text: &str) -> Secret {
        Secret::new(text.to_string())
    }

    /// An account name, as the store would have made it.
    const ADA: &str = "ada";

    /// One account at the one address of one server that greeted.
    fn target() -> Target {
        Target { server: ServerUrl::parse("wss://example.invalid/v1").unwrap(), identity: ADA.into() }
    }

    fn at(phase: Phase) -> SignIn {
        let stage = match phase {
            Phase::Idle => Stage::Idle,
            Phase::Loading => Stage::Loading(None),
            Phase::Trying => Stage::Trying(secret("saved"), Source::Saved),
            Phase::Retrying => Stage::Retrying(secret("saved"), Source::Saved),
            Phase::AskingPassword => Stage::AskingPassword,
            Phase::Submitting => Stage::Submitting,
            Phase::Storing => Stage::Storing(secret("n")),
            Phase::Done => Stage::Done(None),
        };
        SignIn { stage, target: target() }
    }

    #[test]
    fn a_saved_token_is_tried_before_any_password_is_asked() {
        let mut model = SignIn::at(target());
        assert!(matches!(model.update(Input::Begin)[..], [Effect::Go(Errand::LoadToken(_))]));
        assert!(matches!(model.update(Input::TokenLoaded(Some(secret("t"))))[..], [Effect::Go(Errand::TryToken(..))]));
        assert!(matches!(
            model.update(Input::Accepted { token: Some(secret("n")), named: None })[..],
            [Effect::Go(Errand::StoreToken { .. })]
        ));
        assert!(matches!(model.update(Input::Stored(true))[..], [Effect::Done]));
        assert_eq!(model.phase(), Phase::Done);
    }

    #[test]
    fn a_sign_in_that_issues_no_new_token_keeps_the_saved_one() {
        let mut model = at(Phase::Loading);
        assert!(matches!(model.update(Input::TokenLoaded(Some(secret("t"))))[..], [Effect::Go(Errand::TryToken(..))]));
        assert!(matches!(model.update(Input::Accepted { token: None, named: None })[..], [Effect::Done]));
        assert_eq!(model.phase(), Phase::Done, "nothing to store, so nothing is written to the keyring");
    }

    #[test]
    fn with_no_saved_token_the_password_is_asked_and_submitted() {
        let mut model = at(Phase::Loading);
        assert!(matches!(model.update(Input::TokenLoaded(None))[..], [Effect::AskPassword]));
        assert!(matches!(model.update(Input::Password(secret("p")))[..], [Effect::Go(Errand::SubmitPassword(..))]));
        assert_eq!(model.phase(), Phase::Submitting);
    }

    #[test]
    fn an_expired_token_is_forgotten_said_and_replaced_by_a_password() {
        let mut model = at(Phase::Trying);
        let reason = Some(CloseReason::PasswordReset);
        let effects = model.update(Input::Expired(reason));
        assert!(
            matches!(effects[..], [Effect::Go(Errand::ForgetToken(_)), Effect::Expired(said), Effect::AskPassword] if said == reason),
            "the ending the server told is carried, so a person reads why"
        );
        assert_eq!(model.phase(), Phase::AskingPassword);
    }

    #[test]
    fn a_wrong_password_is_asked_again_and_an_unreachable_server_starts_over() {
        let mut model = at(Phase::Submitting);
        let no = Refusal { code: ErrorCode::Unauthenticated };
        assert!(matches!(&model.update(Input::Refused(no))[..], [Effect::Fail(said)] if *said == no.into()));
        assert_eq!(model.phase(), Phase::AskingPassword);
        let mut model = at(Phase::Trying);
        let down = Refusal { code: ErrorCode::Unspecified };
        assert!(matches!(model.update(Input::Refused(down))[..], [Effect::Fail(_)]));
        assert_eq!(model.phase(), Phase::Idle);
    }

    #[test]
    fn a_sign_in_not_begun_reads_no_keyring_and_takes_no_password() {
        assert!(at(Phase::Idle).update(Input::TokenLoaded(Some(secret("t")))).is_empty());
        assert!(at(Phase::Idle).update(Input::Password(secret("p"))).is_empty());
    }

    #[test]
    fn a_password_that_never_got_there_starts_over_without_blaming_it() {
        let mut model = at(Phase::Submitting);
        assert!(matches!(&model.update(Input::Unreached(LinkError::Timeout))[..], [Effect::Fail(LinkError::Timeout)]));
        assert_eq!(model.phase(), Phase::Idle, "nobody's own words are at fault, so the ask starts over");
        let mut waiting = at(Phase::AskingPassword);
        assert!(waiting.update(Input::Unreached(LinkError::Timeout)).is_empty());
    }

    #[test]
    fn a_server_that_did_not_answer_keeps_the_saved_sign_in_and_offers_it_again() {
        let mut model = at(Phase::Loading);
        assert!(
            matches!(model.update(Input::TokenLoaded(Some(secret("saved"))))[..], [Effect::Go(Errand::TryToken(..))]),
            "the token a run read is offered first",
        );
        assert_eq!(model.phase(), Phase::Trying);
        assert!(
            matches!(model.update(Input::Unreached(LinkError::Connect))[..], [Effect::Say(Phrase::SavedSignInHeld)]),
            "the server is the one that did not answer: nothing is deleted and no password is asked for",
        );
        assert_eq!(model.phase(), Phase::Retrying);
        assert!(
            matches!(&model.update(Input::Retry)[..], [Effect::Go(Errand::TryToken(_, one))] if one.expose() == "saved"),
            "the same sign-in is offered again, with no second read of the keyring",
        );
        assert_eq!(model.phase(), Phase::Trying, "a second failure is answered the same way");
    }

    /// It was retried every 30 s for ever: no retry reaches a server of another major.
    #[test]
    fn a_server_of_another_major_is_said_once_and_never_retried() {
        let newer = LinkError::OtherMajor(terms::ServerIs::Newer);
        let mut model = at(Phase::Loading);
        model.update(Input::TokenLoaded(Some(secret("saved"))));
        assert!(matches!(&model.update(Input::Unreached(newer.clone()))[..], [Effect::Fail(said)] if *said == newer));
        assert_eq!(model.phase(), Phase::Idle, "not Retrying, and nothing deleted");
        let mut typed = at(Phase::Submitting);
        assert!(matches!(&typed.update(Input::Unreached(newer.clone()))[..], [Effect::Fail(said)] if *said == newer));
        assert_eq!(words::failure::said(&newer), words::Sentence::Phrase(Phrase::UpdateToJoin));
    }

    /// A host carries each errand as it comes: the model names where it goes.
    #[test]
    fn every_errand_names_the_sign_in_s_target() {
        let mut model = SignIn::at(target());
        let inputs = [
            Input::Begin,
            Input::TokenLoaded(Some(secret("saved"))),
            Input::Expired(None),
            Input::Password(secret("p")),
            Input::Accepted { token: Some(secret("n")), named: None },
        ];
        let asked: Vec<Effect> = inputs.into_iter().flat_map(|input| model.update(input)).collect();
        let at = target();
        assert!(
            matches!(&asked[..], [
                Effect::Go(Errand::LoadToken(a)),
                Effect::Go(Errand::TryToken(b, _)),
                Effect::Go(Errand::ForgetToken(c)),
                Effect::Expired(None),
                Effect::AskPassword,
                Effect::Go(Errand::SubmitPassword(d, _)),
                Effect::Go(Errand::StoreToken { target: e, .. }),
            ] if [a, b, c, d, e].iter().all(|one| **one == at)),
            "{asked:?}"
        );
    }

    #[test]
    fn a_token_that_cannot_be_kept_is_said_and_the_sign_in_still_ends() {
        let mut model = at(Phase::Storing);
        assert!(matches!(
            model.update(Input::Stored(false))[..],
            [Effect::Say(Phrase::KeyringUnavailable), Effect::Done]
        ));
    }

    /// The name the server gave for the account is not the model's to drop: it is the only fact
    /// that makes one keyring entry per account rather than one per casing.
    #[test]
    fn the_name_the_server_gave_reaches_the_ask_to_keep_the_token() {
        let mut model = at(Phase::Submitting);
        let effects = model.update(Input::Accepted { token: Some(secret("n")), named: Some(ADA.into()) });
        assert!(
            matches!(&effects[..], [Effect::Go(Errand::StoreToken { named, .. })] if *named == Some(ADA.into())),
            "the name to keep it under comes back out of the same answer it was read from",
        );
    }

    /// No second sign-in starts while one is under way, and a late answer is ignored.
    #[test]
    fn an_input_out_of_turn_changes_nothing() {
        for phase in [Phase::Loading, Phase::Trying, Phase::Submitting, Phase::Storing, Phase::Done] {
            let mut model = at(phase);
            assert!(model.update(Input::Begin).is_empty(), "{phase:?}");
            assert_eq!(model.phase(), phase);
        }
        let mut model = at(Phase::Submitting);
        assert!(model.update(Input::Password(secret("again"))).is_empty());
        assert!(model.update(Input::Expired(None)).is_empty(), "only a saved token expires");
        assert_eq!(model.phase(), Phase::Submitting);
        let mut model = at(Phase::AskingPassword);
        assert!(model.update(Input::Accepted { token: Some(secret("x")), named: None }).is_empty());
        assert!(model.update(Input::Stored(true)).is_empty());
    }

    fn held() -> SignIn {
        SignIn::holding(target(), secret("held"))
    }

    #[test]
    fn the_token_a_run_holds_is_offered_before_the_keyring_is_read() {
        let mut model = held();
        assert!(
            matches!(&model.update(Input::Begin)[..], [Effect::Go(Errand::TryToken(_, one))] if one.expose() == "held")
        );
        assert_eq!(model.phase(), Phase::Trying);
        assert!(matches!(model.update(Input::Accepted { token: None, named: None })[..], [Effect::Done]));
        assert_eq!(model.held().map(Secret::expose), Some("held"), "the token the server took is the one held");
    }

    #[test]
    fn a_token_the_server_issues_in_its_place_is_the_one_held_once_it_is_kept() {
        let mut model = held();
        model.update(Input::Begin);
        model.update(Input::Accepted { token: Some(secret("issued")), named: None });
        assert_eq!(model.held(), None, "nothing is held before the sign-in is done");
        model.update(Input::Stored(false));
        assert_eq!(model.held().map(Secret::expose), Some("issued"), "held whether or not the keyring kept it");
    }

    #[test]
    fn a_password_that_was_taken_with_no_token_issued_holds_none() {
        let mut model = at(Phase::Submitting);
        model.update(Input::Accepted { token: None, named: None });
        assert_eq!(model.held(), None);
    }

    #[test]
    fn a_held_token_the_server_turns_away_falls_back_to_the_keyring_then_the_password() {
        let mut model = held();
        model.update(Input::Begin);
        assert!(matches!(model.update(Input::Expired(None))[..], [Effect::Go(Errand::LoadToken(_))]));
        assert_eq!(model.phase(), Phase::Loading);
        assert!(
            matches!(&model.update(Input::TokenLoaded(Some(secret("newer"))))[..], [Effect::Go(Errand::TryToken(_, one))] if one.expose() == "newer"),
            "a token the keyring holds that is another is tried"
        );
        let reason = Some(CloseReason::PasswordReset);
        assert!(
            matches!(model.update(Input::Expired(reason))[..], [Effect::Go(Errand::ForgetToken(_)), Effect::Expired(said), Effect::AskPassword] if said == reason),
            "and one refused too is forgotten, then the password is asked"
        );
    }

    #[test]
    fn a_held_token_the_keyring_holds_too_is_forgotten_without_a_second_try() {
        let mut model = held();
        model.update(Input::Begin);
        let reason = Some(CloseReason::PasswordReset);
        model.update(Input::Expired(reason));
        assert!(
            matches!(model.update(Input::TokenLoaded(Some(secret("held"))))[..], [Effect::Go(Errand::ForgetToken(_)), Effect::Expired(said), Effect::AskPassword] if said == reason),
            "the server's reason for the first refusal is the one a person reads"
        );
        assert_eq!(model.phase(), Phase::AskingPassword);
    }

    #[test]
    fn a_held_token_turned_away_with_nothing_in_the_keyring_asks_the_password_and_says_why() {
        let mut model = held();
        model.update(Input::Begin);
        let reason = Some(CloseReason::PasswordReset);
        model.update(Input::Expired(reason));
        assert!(
            matches!(model.update(Input::TokenLoaded(None))[..], [Effect::Expired(said), Effect::AskPassword] if said == reason)
        );
    }

    #[test]
    fn a_held_token_offered_again_after_no_answer_is_still_the_held_one() {
        let mut model = held();
        model.update(Input::Begin);
        model.update(Input::Unreached(LinkError::Connect));
        assert_eq!(model.phase(), Phase::Retrying);
        assert!(
            matches!(&model.update(Input::Retry)[..], [Effect::Go(Errand::TryToken(_, one))] if one.expose() == "held")
        );
        assert!(
            matches!(model.update(Input::Expired(None))[..], [Effect::Go(Errand::LoadToken(_))]),
            "still turned away to the keyring, not forgotten as a saved one"
        );
    }
}

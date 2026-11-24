//! Getting in: a server added by an invite link or an address, a look at it before anything is typed, then an
//! account made with the invite or signed in to, and a password of one's own where the server gave a temporary
//! one. A remembered account opens by itself. Values in, effects out: the host carries out each [`Ask`].

use crate::chosen::{self, LengthNote, MatchNote};
use crate::sign_in::{Effect, Input, Phase, Refusal, Report, SignIn, Standing, Target};
use crate::tell::{Action, Callout, FieldSaid};
use proto::chatkord_v1::{ErrorCode, UsableInvite};
use proto::pin::KeyPin;
use proto::rules::{HandleRefused, handle_shape};
use terms::{Address, AddressError, InviteLink, LinkError, Secret, ServerIs, ServerUrl};
use words::Phrase;
use words::Sentence;

/// A server that greeted: where a credential goes, and what a person reads of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    /// The address that greeted, the route a credential goes by.
    pub url: ServerUrl,
    /// The name it gave itself, if any.
    pub name: Option<String>,
    /// The colour of its picture it picked, if any.
    pub colour: Option<proto::rules::Colour>,
    /// Its picture, if it has one: a file fetched once signed in.
    pub picture: Option<proto::files::FileId>,
}

impl Server {
    /// Where it is, with neither a scheme nor a path: what a person reads of its address.
    pub fn host(&self) -> &str {
        self.url.authority()
    }

    /// Its name, else its address.
    pub fn shown(&self) -> &str {
        self.name.as_deref().unwrap_or_else(|| self.host())
    }

    /// The colour of its picture, as picked: Chatkord's until it picks one.
    pub fn colour(&self) -> design::Rgb {
        self.colour.map_or_else(unpicked, crate::colour::rgb)
    }

    /// What it says of itself now, from a change since it greeted.
    pub(crate) fn said(&mut self, identity: terms::ServerIdentity) {
        (self.name, self.colour, self.picture) = (identity.name, identity.colour, identity.picture);
    }

    /// What it greeted with: its name, colour and picture.
    pub(crate) fn greeted(&mut self, identity: terms::ServerIdentity) {
        self.said(identity);
    }

    /// Its name, colour and picture as they are kept beside its address.
    pub fn identity(&self) -> terms::ServerIdentity {
        terms::ServerIdentity { name: self.name.clone(), colour: self.colour, picture: self.picture }
    }
}

/// The colour of a server that picked none.
fn unpicked() -> design::Rgb {
    design::Accent::DEFAULT.seed()
}

/// Chatkord's colour, as the wire writes one: what a server's picker resets to.
pub(crate) const CHATKORD: proto::rules::Colour = {
    let seed = design::Accent::DEFAULT.seed();
    proto::rules::Colour::new(seed.r, seed.g, seed.b)
};

/// What a server said of an invite before an account is made with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invite {
    /// It admits someone.
    Works {
        /// When it stops admitting, in seconds since the epoch.
        until: i64,
        /// It makes whoever it admits the server's owner.
        owner: bool,
    },
    /// Used up, run out, or never made.
    Refused,
}

impl From<Option<UsableInvite>> for Invite {
    fn from(usable: Option<UsableInvite>) -> Invite {
        match usable {
            Some(usable) => Invite::Works { until: usable.expires_at, owner: usable.owner },
            None => Invite::Refused,
        }
    }
}

/// The host's answer to [`Errand::Find`]: the server that greeted and, when a code was asked about, its standing.
#[derive(Clone, Debug)]
pub struct Found {
    /// The server as it greeted.
    pub server: Server,
    /// What it said of the invite; none when no code was asked about.
    pub invite: Option<Invite>,
}

/// A server whose certificate no authority vouches for, as the first-use prompt shows it: it has not greeted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unvouched {
    /// The host as typed or pasted, which stands for its name.
    pub host: String,
    /// The key it presented.
    pub pin: KeyPin,
}

impl Unvouched {
    /// The colour of its picture: Chatkord's, as it has said none.
    pub fn colour(&self) -> design::Rgb {
        unpicked()
    }
}

/// An account the server took, and what it said of it.
#[derive(Clone, Debug)]
pub struct Admitted {
    /// The token it issued to sign in with next time, if it issued one.
    pub token: Option<Secret>,
    /// The identity the store made for the account, when the server named one: where the token is filed.
    pub named: Option<String>,
    /// What the server said of the account as it let it in.
    pub standing: Standing,
}

/// Which step a person is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// "Add a server": an invite link or an address.
    Add,
    /// The server being looked for.
    Finding,
    /// "Is this the right server?" of a server whose certificate no authority vouches for: its key, to trust or not.
    Trust,
    /// "Is this the right server?"
    Preview,
    /// "Create an account" with the invite: a username, and a password typed twice.
    Create,
    /// A username and password for an account the server has.
    SignIn,
    /// Making the account, signing in or saving a password, with Cancel.
    Working,
    /// A temporary password to replace.
    NewPassword,
    /// A remembered account opening by itself.
    Opening,
    /// The remembered account's server did not answer; its sign-in is kept for another try.
    Retrying,
    /// Signed in.
    Done,
}

/// What is under way on [`Step::Working`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    /// Making an account with the invite.
    Creating,
    /// Signing in with a password, the new one just saved included.
    SigningIn,
    /// Replacing the temporary password.
    SavingPassword,
}

/// What a username field says under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameNote {
    /// The rule, while nothing is wrong.
    Hint,
    /// Spaces typed, with the name they would make with dots.
    Spaces(String),
    /// A character a username may not hold.
    Characters,
    /// Longer than a username may be.
    TooLong,
    /// The server has an account by this name.
    Taken,
}

impl NameNote {
    /// The line under the username field: the rule as help, or what is wrong with the name as a refusal. `server` is
    /// the server's name and `username` what was typed, for a name another account holds.
    pub fn said(&self, server: &str, username: &str) -> FieldSaid {
        match self {
            NameNote::Hint => FieldSaid::help(Sentence::Phrase(Phrase::NameHint)),
            NameNote::Spaces(name) => FieldSaid::refusal(Sentence::Spaces(name.clone())),
            NameNote::Characters => FieldSaid::refusal(Sentence::Phrase(Phrase::NameCharacters)),
            NameNote::TooLong => FieldSaid::refusal(Sentence::NameTooLong),
            NameNote::Taken => {
                FieldSaid::refusal(Sentence::Taken { server: server.to_owned(), name: username.to_owned() })
            }
        }
    }
}

/// Where a door's problem is said: under the field it is about, or in a box of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    /// A refusal under the field.
    Field(FieldSaid),
    /// A box, for a problem about the server rather than a field.
    Callout(Callout),
}

/// What went wrong, said once as a [`Sentence`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Nothing answers at that host, or the name leads nowhere.
    NotFound {
        /// The host as typed or pasted.
        host: String,
    },
    /// This device has no route to any network, so no address could be tried.
    NoNetwork,
    /// What answers there is not a Chatkord server.
    NotChatkord {
        /// The host as typed, or the server's name once it greeted.
        host: String,
    },
    /// Its encryption is not one this device can check.
    Untrusted {
        /// The host as typed, or the server's name once it greeted.
        host: String,
    },
    /// It speaks only a TLS older than 1.3.
    OldEncryption,
    /// It presented another key than the one this device trusts for it; nothing was sent.
    KeyChanged {
        /// The key this device trusts for it.
        trusted: KeyPin,
        /// The key it presented.
        presented: KeyPin,
    },
    /// It answered something this build cannot read.
    Unreadable,
    /// It speaks another major of it, and said which side is behind.
    OtherMajor(ServerIs),
    /// A pasted text shaped like an invite link that is not a working one.
    NotALink,
    /// The server did not answer the ask.
    NoAnswer {
        /// The host as typed, or the server's name once it greeted.
        server: String,
    },
    /// The username or password does not match.
    Wrong {
        /// The server's name, else its address.
        server: String,
    },
    /// The server refused: too many tries too soon.
    TooManyTries,
    /// The server is in this window's list already.
    Already {
        /// The server as the list holds it.
        server: Server,
    },
    /// A refusal nothing above names, by its code.
    Refused(ErrorCode),
}

impl Problem {
    /// The problem as one sentence.
    pub fn sentence(&self) -> Sentence {
        match self {
            Problem::NotFound { host } => Sentence::NotFound(host.clone()),
            Problem::NoNetwork => Sentence::Phrase(Phrase::NoNetwork),
            Problem::NotChatkord { host } => Sentence::NotChatkord(host.clone()),
            Problem::Untrusted { .. } => Sentence::Phrase(Phrase::Untrusted),
            Problem::OldEncryption => Sentence::Phrase(Phrase::OldEncryption),
            Problem::KeyChanged { .. } => Sentence::Phrase(Phrase::KeyChanged),
            Problem::Unreadable => Sentence::Phrase(Phrase::ServerUnreadable),
            Problem::OtherMajor(server) => Sentence::Phrase(words::failure::other_major(*server)),
            Problem::NotALink => Sentence::Phrase(Phrase::NotALink),
            Problem::NoAnswer { server } => Sentence::NoAnswer(server.clone()),
            Problem::Wrong { server } => Sentence::Wrong(server.clone()),
            Problem::TooManyTries => Sentence::Phrase(Phrase::TooManyTries),
            Problem::Already { server } => Sentence::AlreadyListed(server.shown().to_string()),
            Problem::Refused(code) => Sentence::Phrase(words::failure::refusal(*code)),
        }
    }

    /// Where it is said: a server that did not answer, or one this window lists already, is a box; the rest are
    /// refusals under the field.
    pub fn shown(&self) -> Shown {
        match self {
            Problem::NoAnswer { .. } => Shown::Callout(Callout::new(self.sentence()).of(design::Status::Danger)),
            Problem::Already { server } => {
                Shown::Callout(Callout::new(self.sentence()).offering(Action::OpenServer(server.shown().to_string())))
            }
            Problem::NotFound { .. }
            | Problem::NoNetwork
            | Problem::NotChatkord { .. }
            | Problem::Untrusted { .. }
            | Problem::OldEncryption
            | Problem::KeyChanged { .. }
            | Problem::Unreadable
            | Problem::OtherMajor(_)
            | Problem::NotALink
            | Problem::Wrong { .. }
            | Problem::TooManyTries
            | Problem::Refused(_) => Shown::Field(FieldSaid::refusal(self.sentence())),
        }
    }
}

/// What a person does, and what the host reports.
#[derive(Clone, Debug)]
pub enum Message {
    /// The invite link or address field.
    Where(String),
    /// The field's Paste button: the front end reads the clipboard and reports [`Heard::Pasted`].
    Paste,
    /// The username field's whole text after an edit.
    Username(String),
    /// The password field's whole text after an edit.
    Password(Secret),
    /// The whole text of the field that repeats a new password, after an edit.
    Again(Secret),
    /// The password field's eye: show what is typed, or mask it again.
    Reveal,
    /// The repeat field's eye: show what is typed, or mask it again.
    RevealAgain,
    /// The step's main button, or Enter.
    Submit,
    /// Back one step, keeping what was typed.
    Back,
    /// "Create an account" on Sign in, heard only with an invite that works.
    ToCreate,
    /// "I have an account" or "Sign in instead", from the preview or Create.
    ToSignIn,
    /// "Trust this server": its key is trusted, and it is looked for again with it.
    Trust,
    /// "Paste one": back to the first step with nothing typed.
    StartOver,
    /// Cancel while a server is looked for or work is under way: back to the step it came from.
    Cancel,
    /// "Forgot password?", or Ok on its note: opens or closes the note on what to do.
    Forgot,
    /// "Not you? Use another account": the username and password typed are cleared.
    OtherAccount,
    /// "Open `<server>`" for a server already in the list: the window's to do.
    OpenListed,
    /// "Download a debug report" at the foot of the welcome window: the window's to do.
    DebugReport,
    /// The Language field at the foot of the welcome window: the window's to do.
    Language(crate::window::LanguageSaid),
    /// The window opened on this door: a remembered account starts by itself.
    Begin,
    /// The remembered account's server is asked again.
    Retry,
    /// What the host heard, carrying out an errand or reading the clipboard.
    Reported(Heard),
}

/// What happened out in the world.
#[derive(Clone, Debug)]
pub enum Heard {
    /// What the clipboard held.
    Pasted(String),
    /// The answer to [`Errand::Find`].
    Found(Result<Found, LinkError>),
    /// The answer to [`Errand::Register`].
    Registered(Result<Admitted, LinkError>),
    /// The sign-in flow's own reports: the keyring, a token or password sign-in.
    Flow(Report),
    /// The answer to [`Ask::ChangePassword`].
    PasswordSaved(Result<(), LinkError>),
}

/// What the door asks of the host.
#[derive(Debug)]
pub enum Ask {
    /// Replace the temporary password on the signed-in connection; report [`Heard::PasswordSaved`].
    ChangePassword {
        /// The temporary password the sign-in used, which proves the change.
        current: Secret,
        /// The password chosen to replace it.
        new: Secret,
    },
    /// Stop what is under way; its answer, if it comes, is dropped.
    Cancel,
    /// Carry out an errand over the network or the keyring, and report what it heard.
    Go(Errand),
}

/// What reaches the network or the keyring, carried out the same way by every host: a server looked up, an account
/// made, and the errands a sign-in names itself ([`crate::SignIn`]).
#[derive(Debug)]
pub enum Errand {
    /// Greet the address and, with a code, ask whether that invite works, on the same short connection; a server that
    /// greets with `pin` is kept with it.
    Find {
        /// The address the invite link names, or the one typed.
        address: Address,
        /// The invite's code, when a link was given.
        code: Option<Secret>,
        /// The key a person trusted for a server no authority vouches for; none before one is asked about.
        pin: Option<KeyPin>,
    },
    /// Make an account with the invite over a new connection; report [`Heard::Registered`].
    Register {
        /// The address that greeted.
        server: ServerUrl,
        /// The username chosen, as typed.
        identity: String,
        /// The password chosen.
        password: Secret,
        /// The invite's code.
        code: Secret,
    },
    /// Read the token saved for this account at this server; report [`Report::TokenLoaded`].
    LoadToken(Target),
    /// Sign in over a new connection with the saved token; report the server's answer.
    TryToken(Target, Secret),
    /// Delete the saved token; nothing is reported.
    ForgetToken(Target),
    /// Sign in over a new connection with the password; report the server's answer.
    SubmitPassword(Target, Secret),
    /// Keep the token under the identity the server named, when it named one.
    StoreToken {
        /// Where the sign-in went.
        target: Target,
        /// The identity the server named the account by; none to file it under the target's.
        named: Option<String>,
        /// The token the server issued.
        token: Secret,
    },
}

/// A server the window lists, which a server being added is checked against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Listed {
    /// An account is in, or on its way in, there on this device: an address or an invite to it opens it.
    Held(Server),
    /// No account is signed in there on this device: an address opens it to sign in, and an invite goes on to the
    /// account it makes, which the window's place for the server takes.
    Unheld(Server),
}

impl Listed {
    /// The server as the list holds it.
    pub(crate) fn server(&self) -> &Server {
        match self {
            Listed::Held(server) | Listed::Unheld(server) => server,
        }
    }
}

/// What a run before this one knew.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opening {
    /// Nothing: "Add a server".
    Fresh,
    /// A server with no account signed in at it.
    Server(Server),
    /// An account that opens by itself.
    Account(Server, String),
}

/// What a front end shows while a sign-in of a remembered account runs, for a host that only waits on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Showing {
    Form,
    Working,
    Retrying,
    Done,
}

/// How a person arrived, for the note the server opens with; the note names the server as it is when drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arrival {
    /// A new account: others see the username until a name is chosen.
    Joined {
        /// The username, as signed in with.
        name: String,
    },
    /// A new account that owns the server.
    Owner,
}

/// The door: where it is, with what that step holds, and what is typed, which every step keeps.
#[derive(Debug)]
pub struct Door {
    stage: Stage,
    /// What the invite link or address field holds.
    text: String,
    username: String,
    password: Secret,
    again: Secret,
    revealed: bool,
    revealed_again: bool,
    problem: Option<Problem>,
    /// The sentence a remembered account's ending is told with.
    why: Option<Sentence>,
    /// Something to tell that is not a fault: a sign-in kept for another try, a keyring that cannot be used.
    notice: Option<Phrase>,
    /// The servers the window has already, which a server being added is checked against.
    known: Vec<Listed>,
    /// The listed server with no account here that the last look was for, by an invite: its place in the window is
    /// where the errands go and where the account lands. Kept until the next look, as a cancel goes where it went.
    bound: Option<ServerUrl>,
}

/// Looking for a server, or at one that greeted.
#[derive(Debug)]
enum Stage {
    /// "Add a server" or the look under way; the server Back left is kept, so finding it again keeps what was said of
    /// it.
    Looking { step: Looking, left: Option<Box<At>> },
    /// At a server that greeted, or the remembered one.
    At(Box<At>),
}

/// The steps before a server greeted.
#[derive(Debug)]
enum Looking {
    /// "Add a server"; `pasting` while the clipboard is read for the field.
    Add { pasting: bool },
    /// The server being looked for, with the code of the invite link that named it, and the key a person trusted for
    /// it, which takes them past the preview once it greets.
    Finding { code: Option<Secret>, pin: Option<KeyPin> },
    /// A server whose certificate no authority vouches for, its key shown to trust or not.
    Trust { asked: Unvouched },
}

/// A server and the step on it.
#[derive(Debug)]
struct At {
    server: Server,
    /// The invite's code, when a link brought the server.
    code: Option<Secret>,
    /// What the server said of the invite; none without a link.
    invite: Option<Invite>,
    /// "Is this the right server?" was seen, so Back returns to it.
    previewed: bool,
    /// The server has an account by the username typed.
    taken: bool,
    step: AtStep,
    /// The sign-in under way or last made; none before one is asked or a remembered account opens.
    flow: Option<SignIn>,
    /// The temporary password a sign-in used, held to prove its change.
    temporary: Option<Secret>,
    /// What the server said of the account as it let it in; none before it has.
    standing: Option<Standing>,
    account: Account,
}

/// Whether the account signed in to was made here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Account {
    /// One the server had.
    Had,
    /// Made with the invite on this door.
    Made,
}

/// The steps at a server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtStep {
    Preview,
    Create,
    /// `forgot` while the note on a forgotten password is open.
    SignIn {
        forgot: bool,
    },
    Working(Work),
    NewPassword,
    Opening,
    Retrying,
    Done,
}

impl At {
    fn new(server: Server, step: AtStep) -> Box<At> {
        Box::new(At {
            server,
            code: None,
            invite: None,
            previewed: false,
            taken: false,
            step,
            flow: None,
            temporary: None,
            standing: None,
            account: Account::Had,
        })
    }

    fn work(&self) -> Option<Work> {
        match self.step {
            AtStep::Working(work) => Some(work),
            AtStep::Preview
            | AtStep::Create
            | AtStep::SignIn { .. }
            | AtStep::NewPassword
            | AtStep::Opening
            | AtStep::Retrying
            | AtStep::Done => None,
        }
    }
}

impl Default for Door {
    fn default() -> Self {
        Door::opening(Opening::Fresh)
    }
}

fn empty() -> Secret {
    Secret::new(String::new())
}

const SIGN_IN: AtStep = AtStep::SignIn { forgot: false };

impl Door {
    /// Opened on what a run before knew: at "Add a server", at Sign in, or opening a remembered account.
    pub fn opening(what: Opening) -> Door {
        let mut door = Door {
            stage: Stage::Looking { step: Looking::Add { pasting: false }, left: None },
            text: String::new(),
            username: String::new(),
            password: empty(),
            again: empty(),
            revealed: false,
            revealed_again: false,
            problem: None,
            why: None,
            notice: None,
            known: Vec::new(),
            bound: None,
        };
        match what {
            Opening::Fresh => {}
            Opening::Server(server) => {
                door.text = server.host().to_string();
                door.stage = Stage::At(At::new(server, SIGN_IN));
            }
            Opening::Account(server, identity) => door.open_account(server, identity, None),
        }
        door
    }

    /// An account this run was signed in to, opening again by itself after its connection dropped: its sign-in offers
    /// `held`, the token the run signed in with, before the keyring's.
    pub(crate) fn resuming(server: Server, identity: String, held: Option<Secret>) -> Door {
        let mut door = Door::opening(Opening::Fresh);
        door.open_account(server, identity, held);
        door
    }

    /// Opening `identity` at `server` by itself: its sign-in offers `held` first when there is one.
    fn open_account(&mut self, server: Server, identity: String, held: Option<Secret>) {
        self.text = server.host().to_string();
        let target = Target { server: server.url.clone(), identity: identity.clone() };
        let mut at = At::new(server, AtStep::Opening);
        at.flow = Some(match held {
            Some(token) => SignIn::holding(target, token),
            None => SignIn::at(target),
        });
        self.username = identity;
        self.stage = Stage::At(at);
    }

    /// "Add a server" in a window that has `known` already, the username of the account open there typed in it.
    pub(crate) fn adding(known: Vec<Listed>, username: Option<&str>) -> Door {
        Door { known, username: username.map(str::to_string).unwrap_or_default(), ..Door::default() }
    }

    /// An account whose session is over, asked for its password and told why.
    pub fn signed_out(server: Server, identity: String, why: Sentence) -> Door {
        let mut door = Door::opening(Opening::Server(server));
        door.username = identity;
        door.why = Some(why);
        door
    }

    /// Says why the door is shown, in place of what it said.
    pub(crate) fn say_why(&mut self, why: Sentence) {
        self.why = Some(why);
    }

    fn at(&self) -> Option<&At> {
        match &self.stage {
            Stage::At(at) => Some(at),
            Stage::Looking { .. } => None,
        }
    }

    fn at_mut(&mut self) -> Option<&mut At> {
        match &mut self.stage {
            Stage::At(at) => Some(at),
            Stage::Looking { .. } => None,
        }
    }

    /// The server greeted at, or the one Back left: what was said of it outlives a step back.
    fn held(&self) -> Option<&At> {
        match &self.stage {
            Stage::At(at) | Stage::Looking { left: Some(at), .. } => Some(at),
            Stage::Looking { left: None, .. } => None,
        }
    }

    /// The step a person is on, which says what the door shows.
    pub fn step(&self) -> Step {
        match &self.stage {
            Stage::Looking { step: Looking::Add { .. }, .. } => Step::Add,
            Stage::Looking { step: Looking::Finding { .. }, .. } => Step::Finding,
            Stage::Looking { step: Looking::Trust { .. }, .. } => Step::Trust,
            Stage::At(at) => match at.step {
                AtStep::Preview => Step::Preview,
                AtStep::Create => Step::Create,
                AtStep::SignIn { .. } => Step::SignIn,
                AtStep::Working(_) => Step::Working,
                AtStep::NewPassword => Step::NewPassword,
                AtStep::Opening => Step::Opening,
                AtStep::Retrying => Step::Retrying,
                AtStep::Done => Step::Done,
            },
        }
    }

    /// What is under way; none but on [`Step::Working`].
    pub fn work(&self) -> Option<Work> {
        self.at().and_then(At::work)
    }

    /// The server whose key is asked about on [`Step::Trust`].
    pub fn unvouched(&self) -> Option<&Unvouched> {
        match &self.stage {
            Stage::Looking { step: Looking::Trust { asked, .. }, .. } => Some(asked),
            Stage::Looking { .. } | Stage::At(_) => None,
        }
    }

    /// Whether the server being added was named by an invite link, not by an address typed.
    pub fn invited(&self) -> bool {
        InviteLink::claimed(self.text.trim())
    }

    /// The listed server with no account here whose invite the last look was for: the window's place for it takes
    /// what this door asks and the account it makes.
    pub(crate) fn bound(&self) -> Option<&ServerUrl> {
        self.bound.as_ref()
    }

    /// What the invite link or address field holds; a known server's address when the door opens on one.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The server that greeted, or the remembered one; none before one is found.
    pub fn server(&self) -> Option<&Server> {
        self.held().map(|at| &at.server)
    }

    /// The colour of the picture of the server being looked at; Chatkord's before one greeted.
    pub fn server_colour(&self) -> design::Rgb {
        self.server().map_or_else(unpicked, Server::colour)
    }

    /// What the server said of the invite; none without a link, or before it was asked.
    pub fn invite(&self) -> Option<Invite> {
        self.held().and_then(|at| at.invite)
    }

    /// What the username field holds, untrimmed.
    pub fn username(&self) -> &str {
        &self.username
    }

    /// What the password field holds; emptied after a wrong one, and once in.
    pub fn password(&self) -> &Secret {
        &self.password
    }

    /// What the field that repeats a new password holds.
    pub fn again(&self) -> &Secret {
        &self.again
    }

    /// Whether the password field shows what is typed rather than masking it.
    pub fn revealed(&self) -> bool {
        self.revealed
    }

    /// Whether the repeat field shows what is typed rather than masking it.
    pub fn revealed_again(&self) -> bool {
        self.revealed_again
    }

    /// The clipboard is wanted for the field.
    pub fn pasting(&self) -> bool {
        matches!(self.stage, Stage::Looking { step: Looking::Add { pasting: true }, .. })
    }

    /// Whether the note on a forgotten password is open under the field.
    pub fn forgot_open(&self) -> bool {
        self.at().is_some_and(|at| at.step == AtStep::SignIn { forgot: true })
    }

    /// What went wrong last, until something is typed or the step changes; none when nothing did.
    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    /// Why a remembered account is asked for its password, or why its connection is being made again.
    pub fn why(&self) -> Option<&Sentence> {
        self.why.as_ref()
    }

    /// Something to tell that is not a fault, once said.
    pub fn notice(&self) -> Option<Phrase> {
        self.notice
    }

    /// Where the sign-in goes or last went; none before one is asked or a remembered account opens.
    pub fn target(&self) -> Option<&Target> {
        self.held().and_then(|at| at.flow.as_ref()).map(SignIn::target)
    }

    /// Whether the account is in: [`Step::Done`].
    pub fn signed_in(&self) -> bool {
        self.step() == Step::Done
    }

    /// The step a front end draws, for a host that only waits on a remembered account.
    pub(crate) fn showing(&self) -> Showing {
        match self.step() {
            Step::Add | Step::Trust | Step::Preview | Step::Create | Step::SignIn | Step::NewPassword => Showing::Form,
            Step::Finding | Step::Working | Step::Opening => Showing::Working,
            Step::Retrying => Showing::Retrying,
            Step::Done => Showing::Done,
        }
    }

    /// How a person arrived, once in: what the server's first note says.
    pub fn arrival(&self) -> Option<Arrival> {
        let at = self.at().filter(|at| at.step == AtStep::Done && at.account == Account::Made)?;
        Some(if at.standing.as_ref().is_some_and(|standing| standing.place.is_owner()) {
            Arrival::Owner
        } else {
            Arrival::Joined { name: self.username.clone() }
        })
    }

    /// What the server said of the account as it let it in; none before it has.
    pub fn standing(&self) -> Option<Standing> {
        self.held().and_then(|at| at.standing.clone())
    }

    /// The token the sign-in ended holding, once it has: what the session offers first when its connection drops.
    pub(crate) fn token(&self) -> Option<Secret> {
        self.held().and_then(|at| at.flow.as_ref()).and_then(SignIn::held).cloned()
    }

    /// What the username field says under it now.
    pub fn name_note(&self) -> NameNote {
        if self.held().is_some_and(|at| at.taken) {
            return NameNote::Taken;
        }
        match handle_shape(&self.username) {
            Ok(()) | Err(HandleRefused::TooShort) => NameNote::Hint,
            Err(HandleRefused::TooLong) => NameNote::TooLong,
            Err(HandleRefused::Characters) if self.username.contains(char::is_whitespace) => {
                NameNote::Spaces(self.username.split_whitespace().collect::<Vec<_>>().join("."))
            }
            Err(HandleRefused::Characters) => NameNote::Characters,
        }
    }

    /// The length note under a new password, once something is typed.
    pub fn length_note(&self) -> Option<LengthNote> {
        chosen::length_note(&self.password)
    }

    /// The note under the repeated password, once something is typed in it.
    pub fn match_note(&self) -> Option<MatchNote> {
        chosen::match_note(&self.password, &self.again)
    }

    fn chosen_ready(&self) -> bool {
        chosen::ready(&self.password, &self.again)
    }

    /// Whether the step's main button does something now.
    pub fn ready(&self) -> bool {
        match self.step() {
            Step::Add => !self.text.trim().is_empty(),
            Step::Preview => self.invite() != Some(Invite::Refused),
            Step::Create => {
                handle_shape(&self.username).is_ok() && !self.held().is_some_and(|at| at.taken) && self.chosen_ready()
            }
            Step::SignIn => !self.username.trim().is_empty() && !self.password.is_empty(),
            Step::NewPassword => self.chosen_ready(),
            Step::Retrying => true,
            // Trusting a key is a press of its own button, never Enter.
            Step::Trust | Step::Finding | Step::Working | Step::Opening | Step::Done => false,
        }
    }

    /// Whether the preview may go on to making an account: only with an invite that works.
    pub fn can_create(&self) -> bool {
        matches!(self.invite(), Some(Invite::Works { .. }))
    }

    /// Takes what a person did or the host heard and returns what the host must do; a message for a step that is
    /// gone, or a late answer, changes nothing.
    pub fn update(&mut self, message: Message) -> Vec<Ask> {
        let step = self.step();
        match message {
            Message::Where(text) if step == Step::Add => {
                self.text = text;
                self.problem = None;
                Vec::new()
            }
            Message::Paste if step == Step::Add => {
                if let Stage::Looking { step: Looking::Add { pasting }, .. } = &mut self.stage {
                    *pasting = true;
                }
                Vec::new()
            }
            Message::Reported(Heard::Pasted(text)) if self.pasting() => {
                if let Stage::Looking { step: Looking::Add { pasting }, .. } = &mut self.stage {
                    *pasting = false;
                }
                if !text.trim().is_empty() {
                    self.text = text.trim().to_string();
                    self.problem = None;
                }
                Vec::new()
            }
            Message::Username(text) if matches!(step, Step::Create | Step::SignIn) => {
                self.username = text;
                if let Some(at) = self.at_mut() {
                    at.taken = false;
                }
                self.problem = None;
                Vec::new()
            }
            Message::Password(secret) if matches!(step, Step::Create | Step::SignIn | Step::NewPassword) => {
                self.password = secret;
                self.problem = None;
                Vec::new()
            }
            Message::Again(secret) if matches!(step, Step::Create | Step::NewPassword) => {
                self.again = secret;
                Vec::new()
            }
            Message::Reveal => {
                self.revealed = !self.revealed;
                Vec::new()
            }
            Message::RevealAgain => {
                self.revealed_again = !self.revealed_again;
                Vec::new()
            }
            Message::Forgot if step == Step::SignIn => {
                if let Some(at) = self.at_mut() {
                    let open = at.step == AtStep::SignIn { forgot: true };
                    at.step = AtStep::SignIn { forgot: !open };
                }
                Vec::new()
            }
            Message::OtherAccount if step == Step::SignIn => {
                self.username.clear();
                self.password = empty();
                self.go(SIGN_IN);
                Vec::new()
            }
            Message::Submit if self.ready() => self.submit(),
            Message::Back => self.back(),
            Message::ToCreate if step == Step::SignIn && self.can_create() => {
                self.go(AtStep::Create);
                Vec::new()
            }
            Message::ToSignIn if matches!(step, Step::Preview | Step::Create) => {
                self.go(SIGN_IN);
                Vec::new()
            }
            Message::Trust if step == Step::Trust => self.trust(),
            Message::StartOver if step == Step::SignIn => {
                *self = Door { known: std::mem::take(&mut self.known), ..Door::default() };
                Vec::new()
            }
            Message::Cancel => self.cancel(),
            Message::Begin if step == Step::Opening && self.phase() == Some(Phase::Idle) => {
                self.flow_step(Input::Begin)
            }
            Message::Retry if step == Step::Retrying => {
                self.go_quietly(AtStep::Opening);
                self.flow_step(Input::Retry)
            }
            Message::Reported(Heard::Found(found)) if step == Step::Finding => self.found(found),
            Message::Reported(Heard::Registered(admitted)) if self.work() == Some(Work::Creating) => {
                self.registered(admitted)
            }
            Message::Reported(Heard::Flow(report)) => self.flow_report(report),
            Message::Reported(Heard::PasswordSaved(saved)) if self.work() == Some(Work::SavingPassword) => {
                self.saved(saved)
            }
            // A late answer, or a press on a step that is gone.
            Message::Where(_)
            | Message::Paste
            | Message::Username(_)
            | Message::Password(_)
            | Message::Again(_)
            | Message::Forgot
            | Message::OtherAccount
            | Message::Submit
            | Message::ToCreate
            | Message::ToSignIn
            | Message::Trust
            | Message::StartOver
            | Message::Begin
            | Message::Retry
            | Message::OpenListed
            | Message::DebugReport
            | Message::Language(_)
            | Message::Reported(_) => Vec::new(),
        }
    }

    /// To another step at the server, keeping what was typed; what went wrong goes with the step it was said on.
    fn go(&mut self, step: AtStep) {
        self.go_quietly(step);
        self.problem = None;
    }

    /// To another step at the server, keeping what went wrong.
    fn go_quietly(&mut self, step: AtStep) {
        if let Some(at) = self.at_mut() {
            at.step = step;
        }
    }

    /// Back to "Add a server", the server left kept.
    fn back_to_add(&mut self) {
        let left = self.leave();
        self.stage = Stage::Looking { step: Looking::Add { pasting: false }, left };
        self.problem = None;
    }

    /// The server greeted at or left, taken out of the stage.
    fn leave(&mut self) -> Option<Box<At>> {
        let stage =
            std::mem::replace(&mut self.stage, Stage::Looking { step: Looking::Add { pasting: false }, left: None });
        match stage {
            Stage::At(at) => Some(at),
            Stage::Looking { left, .. } => left,
        }
    }

    fn submit(&mut self) -> Vec<Ask> {
        match self.step() {
            Step::Add => self.find(),
            Step::Preview if self.can_create() => {
                self.go(AtStep::Create);
                Vec::new()
            }
            Step::Preview => {
                self.go(SIGN_IN);
                Vec::new()
            }
            Step::Create => self.create(),
            Step::SignIn => self.sign_in(),
            Step::NewPassword => {
                let new = self.password.clone();
                let Some(at) = self.at_mut() else { return Vec::new() };
                let Some(current) = at.temporary.clone() else { return Vec::new() };
                at.step = AtStep::Working(Work::SavingPassword);
                vec![Ask::ChangePassword { current, new }]
            }
            Step::Retrying => {
                self.go_quietly(AtStep::Opening);
                self.flow_step(Input::Retry)
            }
            Step::Trust | Step::Finding | Step::Working | Step::Opening | Step::Done => Vec::new(),
        }
    }

    fn find(&mut self) -> Vec<Ask> {
        self.find_with(None)
    }

    /// The field's server looked for, trusting `pin` for it if it was asked about.
    fn find_with(&mut self, pin: Option<KeyPin>) -> Vec<Ask> {
        let text = self.text.trim();
        let (address, code) = if InviteLink::claimed(text) {
            match InviteLink::parse(text) {
                Ok(link) => (link.address, Some(link.code)),
                Err(_) => {
                    self.problem = Some(Problem::NotALink);
                    return Vec::new();
                }
            }
        } else {
            match Address::typed(text) {
                Ok(address) => (address, None),
                Err(AddressError::Shape) => {
                    self.problem = Some(Problem::NotFound { host: text.to_string() });
                    return Vec::new();
                }
                Err(AddressError::Unprotected) => {
                    self.problem = Some(Problem::Untrusted { host: text.to_string() });
                    return Vec::new();
                }
            }
        };
        // A server in the list is opened, not added twice, and asked nothing; only an invite to one where this device
        // holds no account goes on, to make one there.
        let listed = self.known.iter().find(|listed| address.tried().contains(&listed.server().url));
        self.bound = match (listed, &code) {
            (Some(Listed::Unheld(server)), Some(_)) => Some(server.url.clone()),
            (Some(listed), _) => {
                self.problem = Some(Problem::Already { server: listed.server().clone() });
                return Vec::new();
            }
            (None, _) => None,
        };
        self.problem = None;
        let left = self.leave();
        self.stage = Stage::Looking { step: Looking::Finding { code: code.clone(), pin }, left };
        vec![Ask::Go(Errand::Find { address, code, pin })]
    }

    /// "Trust this server": the key shown is trusted, and the server looked for again with it.
    fn trust(&mut self) -> Vec<Ask> {
        let Some(pin) = self.unvouched().map(|asked| asked.pin) else { return Vec::new() };
        self.find_with(Some(pin))
    }

    fn found(&mut self, found: Result<Found, LinkError>) -> Vec<Ask> {
        let stage =
            std::mem::replace(&mut self.stage, Stage::Looking { step: Looking::Add { pasting: false }, left: None });
        let Stage::Looking { step: Looking::Finding { code, pin }, left } = stage else {
            self.stage = stage;
            return Vec::new();
        };
        match found {
            // The prompt is the preview of a server that has not greeted: what trusting it shows is the step after.
            Ok(found) if pin.is_some() => {
                self.stage = Stage::Looking { step: Looking::Finding { code, pin: None }, left };
                self.found(Ok(found));
                if self.ready() {
                    return self.submit();
                }
            }
            // A key that changed between the prompt and the trust is asked about again.
            Err(LinkError::Unvouched(pin) | LinkError::KeyChanged { presented: pin, .. }) => {
                let asked = Unvouched { host: host_of(&self.text), pin };
                self.stage = Stage::Looking { step: Looking::Trust { asked }, left };
                self.problem = None;
            }
            Ok(found) => {
                // The server before, found again, keeps what was said of it; another is another sign-in.
                let mut at = match left {
                    Some(at) if at.server.url == found.server.url => at,
                    Some(_) | None => At::new(found.server.clone(), AtStep::Preview),
                };
                at.server = found.server;
                at.invite = found.invite;
                at.code = code;
                at.previewed = true;
                at.step = AtStep::Preview;
                self.stage = Stage::At(at);
                self.problem = None;
            }
            Err(failed) => {
                let host = host_of(&self.text);
                self.stage = Stage::Looking { step: Looking::Add { pasting: false }, left };
                self.problem = Some(match failed {
                    LinkError::Connect | LinkError::NoSuchHost => Problem::NotFound { host },
                    LinkError::NoNetwork => Problem::NoNetwork,
                    LinkError::NotChatkord => Problem::NotChatkord { host },
                    LinkError::Untrusted => Problem::Untrusted { host },
                    LinkError::OldEncryption => Problem::OldEncryption,
                    LinkError::Unvouched(_) | LinkError::KeyChanged { .. } => Problem::Untrusted { host },
                    LinkError::Offline | LinkError::Unanswered | LinkError::Lost => {
                        Problem::NoAnswer { server: host.clone() }
                    }
                    LinkError::Malformed => Problem::Unreadable,
                    LinkError::OtherMajor(server) => Problem::OtherMajor(server),
                    LinkError::Refused(code) => Problem::Refused(code),
                    LinkError::Closed(_) | LinkError::Timeout | LinkError::TooLarge => {
                        Problem::NoAnswer { server: host }
                    }
                });
            }
        }
        Vec::new()
    }

    fn create(&mut self) -> Vec<Ask> {
        let (identity, password) = (self.username.clone(), self.password.clone());
        let Some(at) = self.at_mut() else { return Vec::new() };
        let Some(code) = at.code.clone() else { return Vec::new() };
        at.step = AtStep::Working(Work::Creating);
        at.flow = Some(SignIn::submitting(Target { server: at.server.url.clone(), identity: identity.clone() }));
        let server = at.server.url.clone();
        self.problem = None;
        vec![Ask::Go(Errand::Register { server, identity, password, code })]
    }

    fn sign_in(&mut self) -> Vec<Ask> {
        let identity = self.username.trim().to_string();
        let Some(at) = self.at_mut() else { return Vec::new() };
        at.step = AtStep::Working(Work::SigningIn);
        let target = Target { server: at.server.url.clone(), identity };
        self.problem = None;
        self.offer_password(target)
    }

    fn registered(&mut self, admitted: Result<Admitted, LinkError>) -> Vec<Ask> {
        let Some(at) = self.at_mut() else { return Vec::new() };
        match admitted {
            Ok(admitted) => {
                at.account = Account::Made;
                at.standing = Some(admitted.standing);
                self.flow_step(Input::Accepted { token: admitted.token, named: admitted.named })
            }
            Err(LinkError::Refused(ErrorCode::AlreadyExists)) => {
                at.taken = true;
                self.go(AtStep::Create);
                Vec::new()
            }
            Err(LinkError::Refused(ErrorCode::Unauthenticated)) => {
                // The invite stopped working between the look and the ask.
                at.invite = Some(Invite::Refused);
                self.go(AtStep::Preview);
                Vec::new()
            }
            Err(failed) => {
                let problem = failure(&at.server, &failed);
                self.go(AtStep::Create);
                self.problem = Some(problem);
                Vec::new()
            }
        }
    }

    /// A changed password ends every session of the account, this one too, so the new one signs in again.
    fn saved(&mut self, saved: Result<(), LinkError>) -> Vec<Ask> {
        let Some(at) = self.at_mut() else { return Vec::new() };
        match saved {
            Ok(()) => {
                at.temporary = None;
                let Some(target) = at.flow.as_ref().map(|flow| flow.target().clone()) else { return Vec::new() };
                at.step = AtStep::Working(Work::SigningIn);
                self.offer_password(target)
            }
            Err(failed) => {
                let problem = failure(&at.server, &failed);
                self.go(AtStep::NewPassword);
                self.problem = Some(problem);
                Vec::new()
            }
        }
    }

    fn flow_report(&mut self, report: Report) -> Vec<Ask> {
        let password = self.password.clone();
        if let Some(at) = self.at_mut() {
            let signing_in = at.work() == Some(Work::SigningIn);
            let mut problem = None;
            match &report {
                Report::Accepted { standing, .. } => {
                    at.standing = Some(standing.clone());
                    if signing_in && standing.must_change {
                        at.temporary = Some(password);
                    }
                }
                Report::Refused(Refusal { code }) if signing_in => {
                    problem = Some(failure(&at.server, &LinkError::Refused(*code)));
                }
                // About the server's key, its TLS or its protocol, which no try again changes, however the sign-in went.
                Report::Unreached(failed) if failed.kind() == terms::ErrorKind::Lasting => {
                    problem = Some(failure(&at.server, failed));
                }
                Report::Unreached(_) if signing_in => {
                    problem = Some(Problem::NoAnswer { server: at.server.shown().to_string() });
                }
                _ => {}
            }
            if let Some(problem) = problem {
                if matches!(problem, Problem::Wrong { .. }) {
                    self.password = empty();
                }
                self.problem = Some(problem);
            }
        }
        self.flow_step(report.into())
    }

    /// A sign-in at `target` with the password typed, offered now.
    fn offer_password(&mut self, target: Target) -> Vec<Ask> {
        let (flow, errand) = SignIn::with_password(target, self.password.clone());
        if let Some(at) = self.at_mut() {
            at.flow = Some(flow);
        }
        vec![Ask::Go(errand)]
    }

    fn phase(&self) -> Option<Phase> {
        self.at().and_then(|at| at.flow.as_ref()).map(SignIn::phase)
    }

    /// Feeds the sign-in flow and carries out what it asks.
    fn flow_step(&mut self, input: Input) -> Vec<Ask> {
        let Some(at) = self.at_mut() else { return Vec::new() };
        let Some(flow) = at.flow.as_mut() else { return Vec::new() };
        let identity = flow.target().identity.clone();
        let effects = flow.update(input);
        let shown = at.server.shown().to_string();
        let mut todo = Vec::new();
        for effect in effects {
            match effect {
                Effect::Go(errand) => todo.push(Ask::Go(errand)),
                // A remembered account whose token is gone: the sign-in step, saying why.
                Effect::Expired(reason) => self.why = Some(Sentence::Ended { server: shown.clone(), reason }),
                Effect::AskPassword => {
                    self.username = identity.clone();
                    self.go(SIGN_IN);
                }
                Effect::Say(phrase) => self.notice = Some(phrase),
                // The failure was said above, from the report; this only returns to the step it came from.
                Effect::Fail(_) => {
                    if let Some(at) = self.at_mut() {
                        at.step = match at.step {
                            AtStep::Working(Work::Creating) => AtStep::Create,
                            AtStep::Working(Work::SigningIn) | AtStep::Opening => SIGN_IN,
                            other => other,
                        };
                    }
                }
                Effect::Done => self.finish(),
            }
        }
        // The flow keeps a remembered sign-in that went unanswered.
        if self.phase() == Some(Phase::Retrying) {
            self.go_quietly(AtStep::Retrying);
        }
        todo
    }

    fn finish(&mut self) {
        self.password = empty();
        self.again = empty();
        let Some(at) = self.at_mut() else { return };
        if at.temporary.is_some() {
            self.go(AtStep::NewPassword);
            return;
        }
        at.step = AtStep::Done;
        at.code = None;
    }

    fn back(&mut self) -> Vec<Ask> {
        let previewed = self.at().is_some_and(|at| at.previewed);
        match self.step() {
            Step::Trust | Step::Preview => self.back_to_add(),
            Step::Create | Step::SignIn if previewed => self.go(AtStep::Preview),
            Step::Create | Step::SignIn => self.back_to_add(),
            Step::Finding | Step::Working => return self.cancel(),
            Step::Add | Step::NewPassword | Step::Opening | Step::Retrying | Step::Done => {}
        }
        Vec::new()
    }

    fn cancel(&mut self) -> Vec<Ask> {
        if let Stage::Looking { step: step @ Looking::Finding { .. }, left } = &mut self.stage {
            *step = Looking::Add { pasting: false };
            if let Some(flow) = left.as_mut().and_then(|at| at.flow.as_mut()) {
                flow.stop();
            }
            self.problem = None;
            return vec![Ask::Cancel];
        }
        let Some(at) = self.at_mut() else { return Vec::new() };
        let back = match at.step {
            AtStep::Working(Work::Creating) => AtStep::Create,
            AtStep::Working(Work::SigningIn) => SIGN_IN,
            AtStep::Working(Work::SavingPassword) => AtStep::NewPassword,
            AtStep::Preview
            | AtStep::Create
            | AtStep::SignIn { .. }
            | AtStep::NewPassword
            | AtStep::Opening
            | AtStep::Retrying
            | AtStep::Done => return Vec::new(),
        };
        if let Some(flow) = at.flow.as_mut() {
            flow.stop();
        }
        self.go(back);
        vec![Ask::Cancel]
    }
}

/// What a failed ask at `server` is said as.
fn failure(server: &Server, failed: &LinkError) -> Problem {
    let shown = server.shown().to_string();
    match failed {
        LinkError::Refused(ErrorCode::Unauthenticated) => Problem::Wrong { server: shown },
        LinkError::Refused(ErrorCode::RateLimited) => Problem::TooManyTries,
        LinkError::Refused(code) => Problem::Refused(*code),
        LinkError::Connect
        | LinkError::NoSuchHost
        | LinkError::NoNetwork
        | LinkError::Offline
        | LinkError::Unanswered
        | LinkError::Lost
        | LinkError::Closed(_)
        | LinkError::Timeout
        | LinkError::TooLarge => Problem::NoAnswer { server: shown },
        LinkError::NotChatkord => Problem::NotChatkord { host: shown },
        LinkError::Untrusted | LinkError::Unvouched(_) => Problem::Untrusted { host: shown },
        LinkError::KeyChanged { trusted, presented } => {
            Problem::KeyChanged { trusted: *trusted, presented: *presented }
        }
        LinkError::OldEncryption => Problem::OldEncryption,
        LinkError::Malformed => Problem::Unreadable,
        LinkError::OtherMajor(server) => Problem::OtherMajor(*server),
    }
}

/// The word an address reached without encryption is shown with; an encrypted one has none.
pub fn route_warning(url: &ServerUrl) -> Option<Phrase> {
    (!url.encrypted()).then_some(Phrase::RoutePlain)
}

/// The host a person typed or pasted, as an error names it.
fn host_of(text: &str) -> String {
    let text = text.trim();
    let rest = text.split_once("://").map_or(text, |(_, rest)| rest);
    rest.split('/').next().unwrap_or(rest).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::standing_as;
    use proto::rules::MIN_CHOSEN_CHARS;

    const ADA: &str = "Ada";
    const PASSWORD: &str = "evening games with friends";

    fn code() -> String {
        "Rt4wQ9".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6)
    }

    fn link() -> String {
        format!("https://books.example.invalid/join#{}", code())
    }

    fn book_club() -> Server {
        Server {
            url: ServerUrl::parse("wss://books.example.invalid/v1").unwrap(),
            name: Some("Book club".into()),
            colour: None,
            picture: None,
        }
    }

    fn secret(text: &str) -> Secret {
        Secret::new(text.into())
    }

    fn found(invite: Option<Invite>) -> Message {
        Message::Reported(Heard::Found(Ok(Found { server: book_club(), invite })))
    }

    fn works(owner: bool) -> Option<Invite> {
        Some(Invite::Works { until: 1_793_491_200, owner })
    }

    /// A door at "Is this the right server?" for a pasted invite link.
    fn previewed(owner: bool) -> Door {
        let mut door = Door::default();
        door.update(Message::Where(link()));
        let todo = door.update(Message::Submit);
        let [Ask::Go(Errand::Find { code: Some(asked), .. })] = &todo[..] else { panic!("{todo:?}") };
        assert_eq!(asked.expose(), code(), "the link's code is asked about, as a secret");
        door.update(found(works(owner)));
        door
    }

    fn typed_account(door: &mut Door) {
        door.update(Message::Username(ADA.into()));
        door.update(Message::Password(secret(PASSWORD)));
        door.update(Message::Again(secret(PASSWORD)));
    }

    #[test]
    fn a_signed_out_account_is_asked_for_its_password_and_another_account_clears_the_form() {
        let why = Sentence::Ended { server: "Book club".into(), reason: None };
        let mut door = Door::signed_out(book_club(), ADA.into(), why.clone());
        assert_eq!((door.step(), door.username(), door.why()), (Step::SignIn, ADA, Some(&why)));
        door.update(Message::Password(secret(PASSWORD)));
        door.update(Message::Forgot);
        door.update(Message::OtherAccount);
        assert_eq!(door.username(), "");
        assert!(door.password().is_empty() && !door.forgot_open() && !door.ready());
        door.update(Message::Username("Bo".into()));
        door.update(Message::Password(secret(PASSWORD)));
        let todo = door.update(Message::Submit);
        let [Ask::Go(Errand::SubmitPassword(target, _))] = &todo[..] else { panic!("{todo:?}") };
        assert_eq!(target.identity, "Bo", "the other account signs in");
    }

    fn owls() -> Server {
        Server {
            url: ServerUrl::parse("wss://chat.example.invalid/v1").unwrap(),
            name: Some("Night Owls".into()),
            ..book_club()
        }
    }

    #[test]
    fn paste_one_in_the_add_dialog_still_knows_the_servers_listed() {
        let mut door = Door::adding(vec![Listed::Held(owls())], None);
        door.update(Message::Where("books.example.invalid".into()));
        door.update(Message::Submit);
        door.update(found(None));
        door.update(Message::Submit);
        assert_eq!(door.step(), Step::SignIn);
        door.update(Message::StartOver);
        door.update(Message::Where("chat.example.invalid".into()));
        assert!(door.update(Message::Submit).is_empty(), "a listed server is not looked for again");
        assert_eq!(door.problem(), Some(&Problem::Already { server: owls() }));
    }

    #[test]
    fn an_invite_to_a_listed_server_with_no_account_here_goes_on_and_an_address_opens_it() {
        for listed in [Listed::Held(book_club()), Listed::Unheld(book_club())] {
            let mut door = Door::adding(vec![listed.clone()], None);
            door.update(Message::Where("books.example.invalid".into()));
            assert!(door.update(Message::Submit).is_empty(), "an address opens it: {listed:?}");
            assert_eq!(door.problem(), Some(&Problem::Already { server: book_club() }));
        }
        let mut door = Door::adding(vec![Listed::Held(book_club())], None);
        door.update(Message::Where(link()));
        assert!(door.update(Message::Submit).is_empty(), "an account is held there: it opens");
        let mut door = Door::adding(vec![Listed::Unheld(book_club())], None);
        door.update(Message::Where(link()));
        let todo = door.update(Message::Submit);
        assert!(matches!(&todo[..], [Ask::Go(Errand::Find { code: Some(_), .. })]), "{todo:?}");
        assert_eq!(door.bound(), Some(&book_club().url), "the account goes to the place listed");
        door.update(found(works(false)));
        assert!(door.can_create(), "on to the account the invite makes");
        door.update(Message::Back);
        door.update(Message::Where("garden.example.invalid".into()));
        door.update(Message::Submit);
        assert_eq!(door.bound(), None, "another server is another place");
    }

    #[test]
    fn continue_waits_for_something_typed_and_a_pasted_link_finds_its_server() {
        let mut door = Door::default();
        assert!(!door.ready(), "an empty field offers nothing");
        assert!(door.update(Message::Submit).is_empty());
        let door = previewed(false);
        assert_eq!(door.step(), Step::Preview);
        assert_eq!(door.server().unwrap().shown(), "Book club");
        assert!(door.can_create());
    }

    #[test]
    fn paste_reads_the_clipboard_into_the_field() {
        let mut door = Door::default();
        assert!(door.update(Message::Paste).is_empty());
        assert!(door.pasting(), "the front end is asked for the clipboard");
        door.update(Message::Reported(Heard::Pasted(format!("  {}\n", link()))));
        assert_eq!(door.text(), link());
        assert!(!door.pasting());
        door.update(Message::Paste);
        door.update(Message::Reported(Heard::Pasted(String::new())));
        assert_eq!(door.text(), link(), "an empty clipboard changes nothing");
    }

    #[test]
    fn a_broken_link_or_a_server_that_does_not_answer_is_said_on_the_first_step() {
        let mut door = Door::default();
        door.update(Message::Where("https://books.example.invalid/join#short".into()));
        assert!(door.update(Message::Submit).is_empty());
        assert_eq!(door.problem(), Some(&Problem::NotALink));
        door.update(Message::Where("boks.example.invalid".into()));
        assert_eq!(door.problem(), None, "typing clears it");
        door.update(Message::Submit);
        door.update(Message::Reported(Heard::Found(Err(LinkError::NoSuchHost))));
        assert_eq!(door.step(), Step::Add);
        assert_eq!(door.problem(), Some(&Problem::NotFound { host: "boks.example.invalid".into() }));
        assert_eq!(door.text(), "boks.example.invalid", "what was typed stays");
        door.update(Message::Submit);
        door.update(Message::Reported(Heard::Found(Err(LinkError::NotChatkord))));
        assert!(matches!(door.problem(), Some(Problem::NotChatkord { .. })));
    }

    #[test]
    fn an_address_goes_to_sign_in_and_an_owner_invite_says_so() {
        let mut door = Door::default();
        door.update(Message::Where("books.example.invalid".into()));
        let todo = door.update(Message::Submit);
        assert!(matches!(&todo[..], [Ask::Go(Errand::Find { code: None, .. })]));
        door.update(found(None));
        assert!(!door.can_create());
        door.update(Message::Submit);
        assert_eq!(door.step(), Step::SignIn);
        assert!(matches!(previewed(true).invite(), Some(Invite::Works { owner: true, .. })));
    }

    #[test]
    fn an_invite_that_does_not_work_offers_only_signing_in() {
        let mut door = Door::default();
        door.update(Message::Where(link()));
        door.update(Message::Submit);
        door.update(found(Some(Invite::Refused)));
        assert!(!door.ready(), "Continue is not offered");
        door.update(Message::ToSignIn);
        assert_eq!(door.step(), Step::SignIn);
    }

    #[test]
    fn create_waits_for_a_good_name_and_two_passwords_that_match_long_enough_and_uncommon() {
        let mut door = previewed(false);
        door.update(Message::Submit);
        assert_eq!(door.step(), Step::Create);
        door.update(Message::Username("Ada Lovelace".into()));
        assert_eq!(door.name_note(), NameNote::Spaces("Ada.Lovelace".into()));
        door.update(Message::Username("Ada!".into()));
        assert_eq!(door.name_note(), NameNote::Characters);
        door.update(Message::Username(ADA.into()));
        assert_eq!(door.name_note(), NameNote::Hint);
        door.update(Message::Password(secret("short")));
        assert_eq!(door.length_note(), Some(LengthNote::ToGo(MIN_CHOSEN_CHARS - 5)));
        door.update(Message::Password(secret("PasswordPassword")));
        door.update(Message::Again(secret("PasswordPassword")));
        assert_eq!(door.length_note(), Some(LengthNote::Common));
        assert!(!door.ready(), "a common password is not offered to the server");
        door.update(Message::Password(secret(PASSWORD)));
        assert_eq!(door.length_note(), Some(LengthNote::LongEnough));
        door.update(Message::Again(secret("evening games with friend")));
        assert_eq!(door.match_note(), Some(MatchNote::Differs));
        assert!(!door.ready());
        assert!(door.update(Message::Submit).is_empty(), "two passwords that differ ask nothing");
        door.update(Message::Again(secret(PASSWORD)));
        assert!(door.ready());
        let todo = door.update(Message::Submit);
        let [Ask::Go(Errand::Register { identity, code: asked, .. })] = &todo[..] else { panic!("{todo:?}") };
        assert_eq!(identity, ADA, "the capital is kept as typed");
        assert_eq!(asked.expose(), code());
        assert_eq!(door.step(), Step::Working);
    }

    #[test]
    fn a_taken_name_says_so_and_hands_the_name_to_sign_in() {
        let mut door = previewed(false);
        door.update(Message::Submit);
        typed_account(&mut door);
        door.update(Message::Submit);
        let taken = LinkError::Refused(ErrorCode::AlreadyExists);
        door.update(Message::Reported(Heard::Registered(Err(taken))));
        assert_eq!(door.step(), Step::Create);
        assert_eq!(door.name_note(), NameNote::Taken);
        assert!(!door.ready());
        door.update(Message::ToSignIn);
        assert_eq!(door.step(), Step::SignIn);
        assert_eq!(door.username(), ADA);
    }

    #[test]
    fn an_account_made_is_kept_and_opens_with_its_note() {
        let mut door = previewed(true);
        door.update(Message::Submit);
        typed_account(&mut door);
        door.update(Message::Submit);
        let admitted = Admitted {
            token: Some(secret("t")),
            named: Some(ADA.into()),
            standing: Standing { place: proto::rules::Place::OWNER, ..standing_as("ada") },
        };
        let todo = door.update(Message::Reported(Heard::Registered(Ok(admitted))));
        assert!(matches!(&todo[..], [Ask::Go(Errand::StoreToken { .. })]), "{todo:?}");
        door.update(Message::Reported(Heard::Flow(Report::Stored(true))));
        assert!(door.signed_in());
        assert_eq!(door.arrival(), Some(Arrival::Owner));
        assert!(door.password().is_empty() && door.again().is_empty(), "nothing typed is held once in");
    }

    #[test]
    fn a_wrong_password_clears_it_and_keeps_the_name() {
        let mut door = Door::opening(Opening::Server(book_club()));
        door.update(Message::Username(ADA.into()));
        door.update(Message::Password(secret("not it")));
        assert!(matches!(&door.update(Message::Submit)[..], [Ask::Go(Errand::SubmitPassword(..))]));
        let refused = Refusal { code: ErrorCode::Unauthenticated };
        door.update(Message::Reported(Heard::Flow(Report::Refused(refused))));
        assert_eq!(door.step(), Step::SignIn);
        assert_eq!(door.problem(), Some(&Problem::Wrong { server: "Book club".into() }));
        assert!(door.password().is_empty());
        assert_eq!(door.username(), ADA);
    }

    #[test]
    fn a_temporary_password_is_replaced_before_the_server_opens() {
        let mut door = Door::opening(Opening::Server(book_club()));
        door.update(Message::Username(ADA.into()));
        door.update(Message::Password(secret("river-copper-lamp-42")));
        door.update(Message::Submit);
        let standing = Standing { must_change: true, ..standing_as("ada") };
        let todo = door.update(Message::Reported(Heard::Flow(Report::Accepted {
            token: Some(secret("t")),
            named: Some(ADA.into()),
            standing,
        })));
        assert!(matches!(&todo[..], [Ask::Go(Errand::StoreToken { .. })]));
        door.update(Message::Reported(Heard::Flow(Report::Stored(true))));
        assert_eq!(door.step(), Step::NewPassword);
        door.update(Message::Password(secret(PASSWORD)));
        door.update(Message::Again(secret(PASSWORD)));
        let todo = door.update(Message::Submit);
        let [Ask::ChangePassword { current, new }] = &todo[..] else { panic!("{todo:?}") };
        assert_eq!(current.expose(), "river-copper-lamp-42");
        assert_eq!(new.expose(), PASSWORD);
        let todo = door.update(Message::Reported(Heard::PasswordSaved(Ok(()))));
        let [Ask::Go(Errand::SubmitPassword(_, again))] = &todo[..] else { panic!("{todo:?}") };
        assert_eq!(again.expose(), PASSWORD, "the change ended every session, so the new password signs in");
        let accepted = Report::Accepted { token: Some(secret("u")), named: None, standing: standing_as("ada") };
        door.update(Message::Reported(Heard::Flow(accepted)));
        door.update(Message::Reported(Heard::Flow(Report::Stored(true))));
        assert!(door.signed_in());
    }

    #[test]
    fn only_a_route_without_encryption_carries_a_word() {
        assert_eq!(route_warning(&ServerUrl::parse("ws://127.0.0.1:8443/v1").unwrap()), Some(Phrase::RoutePlain));
        assert_eq!(route_warning(&book_club().url), None, "an encrypted route says nothing more");
    }

    #[test]
    fn back_from_signing_in_returns_to_the_server_seen_or_to_the_start() {
        let mut door = Door::default();
        door.update(Message::Where("books.example.invalid".into()));
        door.update(Message::Submit);
        door.update(found(None));
        door.update(Message::Submit);
        door.update(Message::Back);
        assert_eq!(door.step(), Step::Preview, "an address's preview was seen");
        let mut remembered = Door::opening(Opening::Server(book_club()));
        remembered.update(Message::Back);
        assert_eq!(remembered.step(), Step::Add, "a remembered server had no preview");
        assert_eq!(remembered.text(), "books.example.invalid");
    }

    #[test]
    fn back_keeps_what_was_typed_and_cancel_returns_to_the_step() {
        let mut door = previewed(false);
        door.update(Message::Submit);
        typed_account(&mut door);
        door.update(Message::Back);
        assert_eq!(door.step(), Step::Preview);
        door.update(Message::Back);
        assert_eq!(door.step(), Step::Add);
        assert_eq!(door.text(), link());
        door.update(Message::Submit);
        door.update(found(works(false)));
        door.update(Message::Submit);
        assert_eq!(door.username(), ADA, "the name typed before is still there");
        door.update(Message::Submit);
        assert_eq!(door.step(), Step::Working);
        assert!(matches!(door.update(Message::Cancel)[..], [Ask::Cancel]));
        assert_eq!(door.step(), Step::Create);
        let late = Admitted { token: None, named: None, standing: standing_as("ada") };
        assert!(door.update(Message::Reported(Heard::Registered(Ok(late)))).is_empty(), "a late answer is dropped");
        assert!(!door.signed_in());
    }

    #[test]
    fn a_remembered_account_opens_by_itself_and_one_whose_token_ended_says_why() {
        let mut door = Door::opening(Opening::Account(book_club(), ADA.into()));
        assert_eq!(door.step(), Step::Opening);
        assert!(matches!(door.update(Message::Begin)[..], [Ask::Go(Errand::LoadToken(_))]));
        let todo = door.update(Message::Reported(Heard::Flow(Report::TokenLoaded(Some(secret("t"))))));
        assert!(matches!(&todo[..], [Ask::Go(Errand::TryToken(..))]));
        let reason = Some(proto::chatkord_v1::CloseReason::PasswordReset);
        let todo = door.update(Message::Reported(Heard::Flow(Report::Expired(reason))));
        assert!(matches!(&todo[..], [Ask::Go(Errand::ForgetToken(_))]));
        assert_eq!(door.step(), Step::SignIn);
        assert_eq!(door.why(), Some(&Sentence::Ended { server: "Book club".into(), reason }));
        assert_eq!(door.username(), ADA);
    }

    #[test]
    fn a_remembered_server_that_does_not_answer_keeps_the_sign_in_for_another_try() {
        let mut door = Door::opening(Opening::Account(book_club(), ADA.into()));
        door.update(Message::Begin);
        door.update(Message::Reported(Heard::Flow(Report::TokenLoaded(Some(secret("t"))))));
        door.update(Message::Reported(Heard::Flow(Report::Unreached(LinkError::Connect))));
        assert_eq!(door.showing(), Showing::Retrying);
        assert!(matches!(door.update(Message::Retry)[..], [Ask::Go(Errand::TryToken(..))]));
    }
    fn pin(byte: u8) -> KeyPin {
        KeyPin::from_digest([byte; KeyPin::LEN])
    }

    /// A door asked about the key of a server that typed as `lan.example.invalid` presented, no authority vouching.
    fn asked_to_trust() -> Door {
        let mut door = Door::default();
        door.update(Message::Where("lan.example.invalid".into()));
        let todo = door.update(Message::Submit);
        assert!(
            matches!(&todo[..], [Ask::Go(Errand::Find { pin: None, .. })]),
            "nothing is trusted before it is asked"
        );
        door.update(Message::Reported(Heard::Found(Err(LinkError::Unvouched(pin(7))))));
        door
    }

    #[test]
    fn a_server_no_authority_vouches_for_shows_its_key_to_trust_by_its_address() {
        let door = asked_to_trust();
        assert_eq!(door.step(), Step::Trust);
        assert_eq!(door.unvouched(), Some(&Unvouched { host: "lan.example.invalid".into(), pin: pin(7) }));
        assert_eq!((door.problem(), door.server()), (None, None), "it has not greeted, so it has no name");
        assert!(!door.ready(), "Enter trusts nothing");
    }

    #[test]
    fn trusting_the_key_looks_for_the_server_again_with_it_and_goes_past_the_preview() {
        let mut door = asked_to_trust();
        let todo = door.update(Message::Trust);
        let [Ask::Go(Errand::Find { address, code: None, pin: Some(trusted) })] = &todo[..] else { panic!("{todo:?}") };
        assert_eq!((address.tried()[0].authority(), *trusted), ("lan.example.invalid", pin(7)));
        assert_eq!(door.step(), Step::Finding);
        door.update(found(None));
        assert_eq!(door.step(), Step::SignIn, "the prompt was the preview: signing in is next");
        assert!(door.update(Message::Back).is_empty());
        assert_eq!(door.step(), Step::Preview, "back is the server as it greeted");
    }

    #[test]
    fn back_from_the_key_returns_to_the_address_as_typed() {
        let mut door = asked_to_trust();
        assert!(door.update(Message::Back).is_empty());
        assert_eq!((door.step(), door.text(), door.unvouched()), (Step::Add, "lan.example.invalid", None));
        assert!(door.update(Message::Trust).is_empty(), "a trust after Back is too late");
    }

    #[test]
    fn a_key_that_changed_before_it_was_trusted_is_asked_about_again() {
        let mut door = asked_to_trust();
        door.update(Message::Trust);
        let changed = LinkError::KeyChanged { trusted: pin(7), presented: pin(8) };
        door.update(Message::Reported(Heard::Found(Err(changed))));
        assert_eq!(door.step(), Step::Trust);
        assert_eq!(door.unvouched().map(|asked| asked.pin), Some(pin(8)));
    }

    #[test]
    fn a_server_speaking_only_an_older_tls_says_so_on_the_first_step() {
        let mut door = Door::default();
        door.update(Message::Where("old.example.invalid".into()));
        door.update(Message::Submit);
        door.update(Message::Reported(Heard::Found(Err(LinkError::OldEncryption))));
        assert_eq!((door.step(), door.problem()), (Step::Add, Some(&Problem::OldEncryption)));
    }

    #[test]
    fn a_key_that_changed_at_a_password_sign_in_is_said_with_both_keys() {
        let mut door = Door::signed_out(book_club(), ADA.into(), Sentence::Text(words::Text::TokenLeft));
        door.update(Message::Password(secret(PASSWORD)));
        door.update(Message::Submit);
        let changed = LinkError::KeyChanged { trusted: pin(1), presented: pin(2) };
        door.update(Message::Reported(Heard::Flow(Report::Unreached(changed))));
        assert_eq!(door.problem(), Some(&Problem::KeyChanged { trusted: pin(1), presented: pin(2) }));
        assert_eq!(door.step(), Step::SignIn);
    }
}

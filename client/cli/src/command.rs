//! What the person asked for, parsed once from the arguments.

use proto::chatkord_v1::ChannelAccess;
use proto::rules::{Colour, Grant, Overwrite};
use terms::{Address, ChannelId};
use words::{Phrase, Sentence};

/// One run of the command line.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Something done on a server as one of its accounts.
    As {
        /// The account, and the server it is on.
        account: Account,
        /// What is done.
        act: Act,
    },
    /// Says whether an invite works on the server at this address, signed in as no one.
    InviteCheck(Address),
    /// Lists the microphones and speakers this device has, and which is chosen.
    Devices,
    /// Uses the microphone of this id, as `devices` lists it, for calls; none is the default one.
    UseMicrophone(Option<String>),
    /// Uses the speaker of this id, as `devices` lists it, for calls; none is the default one.
    UseSpeaker(Option<String>),
    /// Picks a screen and says what it is, sharing nothing.
    Pick(share::Want),
    /// Picks a screen and encodes a few seconds of it, saying which encoder took it; nothing is sent.
    Capture(share::Want),
}

/// An account as the command line names it: the server's address, and the account's name there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// Where the server is.
    pub address: Address,
    /// The account's name on it, as typed.
    pub identity: String,
}

/// What is done as an account. An id is the server's, as `channels`, `invite list` and `member list` print it.
#[derive(Debug, PartialEq, Eq)]
pub enum Act {
    /// Makes the account with an invite, with this display name or none, and keeps its session's token.
    Register(Option<String>),
    /// Signs in and keeps the session's token in the keyring.
    SignIn,
    /// Lists the server's channels.
    Channels,
    /// Joins a voice channel and sends the microphone until stopped.
    Speak {
        /// The channel's id.
        channel_id: ChannelId,
        /// A steady tone is sent in place of the microphone.
        tone: bool,
    },
    /// Joins the voice channel with this id and plays what it hears until stopped.
    Listen(ChannelId),
    /// Joins a voice channel and shares what is picked until stopped.
    Share {
        /// What the picker offers.
        want: share::Want,
        /// The applications whose sound goes along, as typed: a name or a program.
        sound: Vec<String>,
        /// The channel's id.
        channel_id: ChannelId,
    },
    /// Joins a voice channel as the window does and says each second how each screen shared in it arrives, or writes
    /// one as it was sent, until stopped.
    Watch {
        /// The channel's id.
        channel_id: ChannelId,
        /// Where the screen goes.
        to: WatchTo,
    },
    /// Ends this device's session and forgets its token.
    SignOut,
    /// Changes the password.
    ChangePassword,
    /// Makes an invite with these terms.
    InviteCreate(InviteTerms),
    /// Lists the server's invites.
    InviteList,
    /// Revokes the invite with this id.
    InviteRevoke(String),
    /// Ends the sessions of the member with this id and prints a temporary password for them.
    MemberReset(String),
    /// Lists the server's members.
    MemberList,
    /// Gives a member a role, or takes it away.
    MemberRole {
        /// The member's id.
        user_id: String,
        /// The role's name.
        role: String,
        /// The member holds the role afterwards (`give`), or does not (`take`).
        held: bool,
    },
    /// Signs the member with this id out of every session they hold.
    MemberKick(String),
    /// Bans the member with this id, or unbans them, proved by the asker's password.
    MemberBan {
        /// The member's id.
        user_id: String,
        /// Bans (`ban`), else unbans (`unban`).
        banned: bool,
    },
    /// Lists the server's roles, each with its place in the order and its words.
    RoleList,
    /// Makes a role, holding no word, just above the default role.
    RoleCreate {
        /// Its name.
        name: String,
        /// Its colour; none for none.
        colour: Option<Colour>,
    },
    /// Renames a role.
    RoleRename {
        /// The role's name now.
        role: String,
        /// Its new name.
        name: String,
    },
    /// Colours a role, or takes its colour off.
    RoleColour {
        /// The role's name.
        role: String,
        /// Its colour; none takes it off.
        colour: Option<Colour>,
    },
    /// Moves a role in the order.
    RoleMove {
        /// The role's name.
        role: String,
        /// The role it goes just above, by name; none for just above the default role.
        before: Option<String>,
    },
    /// Deletes the role with this name.
    RoleDelete(String),
    /// Gives a role a grant word, or takes it.
    RoleWord {
        /// The role's name.
        role: String,
        /// The word.
        word: Grant,
        /// The role holds the word afterwards (`give`), or does not (`take`).
        held: bool,
    },
    /// Makes the member with this id the server's owner.
    OwnerTransfer(String),
    /// Leaves the server.
    Leave,
    /// Makes a channel.
    ChannelCreate {
        /// A voice channel, else a text one.
        voice: bool,
        /// Its name.
        name: String,
        /// The category it goes in, last, by id.
        category: String,
        /// Who sees or writes in it from its start; none leaves it to the roles.
        access: Option<Access>,
    },
    /// Sets the server's picture from a file, or with none takes it off.
    ServerPicture(Option<String>),
    /// Deletes the channel with this id.
    ChannelDelete(String),
    /// Moves a channel, as the list read just before says it stands.
    ChannelMove {
        /// The channel's id.
        channel_id: String,
        /// The category it goes in, by id.
        category: String,
        /// The channel it goes just before, by id; none for last.
        before: Option<String>,
    },
    /// Lists what each role and member may do in the channel with this id beside the roles' own words.
    ChannelOverwrites(String),
    /// Sets what a role or a member may do with one word in a channel.
    ChannelOverwrite {
        /// The channel's id.
        channel_id: String,
        /// The role or member it is for.
        whom: Whom,
        /// The word, one a channel takes.
        word: Grant,
        /// Allowed or denied there; none leaves it to the role's own words (`inherit`).
        overwrite: Option<Overwrite>,
    },
    /// Makes a category with this name, last in the order.
    CategoryCreate(String),
    /// Renames a category.
    CategoryRename {
        /// The category's id.
        category_id: String,
        /// Its new name.
        name: String,
    },
    /// Deletes the category with this id, with its channels and their messages.
    CategoryDelete(String),
    /// Moves a category, as the list read just before says it stands.
    CategoryMove {
        /// The category's id.
        category_id: String,
        /// The category it goes just before, by id; none for last.
        before: Option<String>,
    },
}

/// Who a channel's overwrite is for, as typed.
#[derive(Debug, PartialEq, Eq)]
pub enum Whom {
    /// A role, by its name.
    Role(String),
    /// A member, by their id.
    Member(String),
}

/// Who sees or writes in a channel made from the command line, from its start.
#[derive(Debug, PartialEq, Eq)]
pub struct Access {
    /// What the default role is denied there; never `Unspecified`.
    pub kept: ChannelAccess,
    /// The role allowed what the default role is denied, by name; the maker is allowed it anyway.
    pub role: Option<String>,
}

/// What an invite made from the command line allows; the server checks the bounds.
#[derive(Debug, PartialEq, Eq)]
pub struct InviteTerms {
    /// How many times it can be used.
    pub uses: u32,
    /// The days until it lapses.
    pub days: u32,
    /// The role it gives, by name.
    pub role: Option<String>,
    /// A note kept with it.
    pub note: Option<String>,
}

impl Default for InviteTerms {
    fn default() -> Self {
        Self { uses: 1, days: 7, role: None, note: None }
    }
}

/// `--uses N`, `--days D`, `--role NAME` and `--note TEXT`, in any order, each at most once.
fn invite_terms(flags: &[&str]) -> Result<InviteTerms, String> {
    let mut terms = InviteTerms::default();
    let mut seen = Vec::new();
    for pair in flags.chunks(2) {
        let [flag, value] = pair else { return Err(USAGE.to_string()) };
        if seen.contains(flag) || value.is_empty() {
            return Err(USAGE.to_string());
        }
        seen.push(*flag);
        match *flag {
            "--uses" => terms.uses = value.parse().map_err(|_| USAGE.to_string())?,
            "--days" => terms.days = value.parse().map_err(|_| USAGE.to_string())?,
            "--role" => terms.role = Some(value.to_string()),
            "--note" => terms.note = Some(value.to_string()),
            _ => return Err(USAGE.to_string()),
        }
    }
    Ok(terms)
}

/// The value of each flag `names` lists, given as the flag and then its value, in any order, each at most once.
fn flags<const N: usize>(words: &[&str], names: [&str; N]) -> Result<[Option<String>; N], String> {
    let mut values = [const { None }; N];
    for pair in words.chunks(2) {
        let [flag, value] = pair else { return Err(USAGE.to_string()) };
        let slot = names.iter().position(|name| name == flag).and_then(|at| values.get_mut(at));
        let Some(slot @ None) = slot else { return Err(USAGE.to_string()) };
        if value.is_empty() {
            return Err(USAGE.to_string());
        }
        *slot = Some((*value).to_string());
    }
    Ok(values)
}

/// `--category CATEGORY [--access private|read-only] [--role ROLE]`: where a channel is made and who sees or writes
/// in it; a role is named only beside an access, and only a text channel is read-only.
fn channel_create(voice: bool, name: String, words: &[&str]) -> Result<Act, String> {
    let [category, kept, role] = flags(words, ["--category", "--access", "--role"])?;
    let category = category.ok_or_else(|| USAGE.to_string())?;
    let kept = match kept.as_deref() {
        None => None,
        Some("private") => Some(ChannelAccess::Private),
        Some("read-only") if voice => return Err("A voice channel cannot be read-only.".to_string()),
        Some("read-only") => Some(ChannelAccess::ReadOnly),
        Some(_) => return Err(USAGE.to_string()),
    };
    let access = match (kept, role) {
        (Some(kept), role) => Some(Access { kept, role }),
        (None, None) => None,
        (None, Some(_)) => return Err(USAGE.to_string()),
    };
    Ok(Act::ChannelCreate { voice, name, category, access })
}

/// `role ROLE|member MEMBER WORD allow|deny|inherit`: what a role or a member may do with a word in a channel;
/// `inherit` is no overwrite, so the roles' own words decide.
fn channel_overwrite(channel_id: String, kind: &str, who: String, word: &str, state: &str) -> Result<Act, String> {
    let whom = match kind {
        "role" => Whom::Role(who),
        "member" => Whom::Member(who),
        _ => return Err(USAGE.to_string()),
    };
    let overwrite = match state {
        "allow" | "deny" | "inherit" => Overwrite::of_word(state),
        _ => return Err(USAGE.to_string()),
    };
    Ok(Act::ChannelOverwrite { channel_id, whom, word: channel_word(word)?, overwrite })
}

/// A colour as typed, `#RRGGBB` or `RRGGBB`; anything else is the colour field's own hint.
fn colour(text: &str) -> Result<Colour, String> {
    Colour::parse(text).map_err(|_| crate::english(&Sentence::Phrase(Phrase::ColourHexHint)))
}

/// A grant word as `role list` prints it, in either case.
fn grant(text: &str) -> Result<Grant, String> {
    Grant::of_word(&text.to_ascii_uppercase()).ok_or_else(|| format!("No word is called {text}."))
}

/// A grant word a channel's overwrites take.
fn channel_word(text: &str) -> Result<Grant, String> {
    let word = grant(text)?;
    if word.in_channel() { Ok(word) } else { Err(format!("A channel does not take {}.", word.word())) }
}

/// `[monitor|window] [--sound APP]... ADDRESS NAME CHANNEL`: what a share takes, and the sound that goes along.
fn share_words<'a>(words: &'a [&'a str]) -> Result<(share::Want, Vec<String>, [&'a str; 3]), String> {
    let (want, mut rest) = match words {
        ["monitor", rest @ ..] => (share::Want::Monitor, rest),
        ["window", rest @ ..] => (share::Want::Window, rest),
        rest => (share::Want::Either, rest),
    };
    let mut sound = Vec::new();
    while let ["--sound", app, more @ ..] = rest {
        if app.is_empty() || sound.iter().any(|chosen| chosen == app) {
            return Err(USAGE.to_string());
        }
        sound.push((*app).to_string());
        rest = more;
    }
    match rest {
        [u, n, c] => Ok((want, sound, [*u, *n, *c])),
        _ => Err(USAGE.to_string()),
    }
}

/// Where a watched screen goes.
#[derive(Debug, PartialEq, Eq)]
pub enum WatchTo {
    /// Nowhere: the screens are decoded as the window decodes them, and each second says how they arrive.
    Reports,
    /// The screen as the sender encoded it (H.264, Annex B), written to this file without decoding it here.
    Stream(String),
}

/// An address is asked for as `ADDRESS`, not `URL`: nobody is asked to write a scheme.
pub(crate) const USAGE: &str = "usage: chatkord register ADDRESS NAME [DISPLAY_NAME] | sign-in ADDRESS NAME | channels ADDRESS NAME | speak ADDRESS NAME CHANNEL [--tone] | listen ADDRESS NAME CHANNEL | devices | use-microphone ID|--default | use-speaker ID|--default | pick [monitor|window] | capture [monitor|window] | share [monitor|window] [--sound APP]... ADDRESS NAME CHANNEL | watch ADDRESS NAME CHANNEL [--stream FILE] | sign-out ADDRESS NAME | change-password ADDRESS NAME | invite create ADDRESS NAME [--uses N] [--days D] [--role ROLE] [--note TEXT] | invite list ADDRESS NAME | invite revoke ADDRESS NAME INVITE | invite check ADDRESS | member reset ADDRESS NAME MEMBER | member list ADDRESS NAME | member role ADDRESS NAME MEMBER ROLE give|take | member kick ADDRESS NAME MEMBER | member ban|unban ADDRESS NAME MEMBER | role list ADDRESS NAME | role create ADDRESS NAME ROLE_NAME [COLOUR] | role rename ADDRESS NAME ROLE ROLE_NAME | role colour ADDRESS NAME ROLE [COLOUR] | role move ADDRESS NAME ROLE [BEFORE_ROLE] | role delete ADDRESS NAME ROLE | role word ADDRESS NAME ROLE WORD give|take | owner transfer ADDRESS NAME MEMBER | leave ADDRESS NAME | channel create ADDRESS NAME voice|text CHANNEL_NAME --category CATEGORY [--access private|read-only] [--role ROLE] | channel delete ADDRESS NAME CHANNEL | channel move ADDRESS NAME CHANNEL --category CATEGORY [--before CHANNEL] | channel overwrites ADDRESS NAME CHANNEL | channel overwrite ADDRESS NAME CHANNEL role ROLE|member MEMBER WORD allow|deny|inherit | category create ADDRESS NAME CATEGORY_NAME | category rename ADDRESS NAME CATEGORY CATEGORY_NAME | category delete ADDRESS NAME CATEGORY | category move ADDRESS NAME CATEGORY [BEFORE_CATEGORY] | server picture ADDRESS NAME FILE|--none";

/// The command the arguments name, or the usage line when they name none, or an address's fault in a sentence.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let address = |text: &str| Address::typed(text).map_err(|e| e.to_string());
    let name = |text: &str| if text.is_empty() { Err(USAGE.to_string()) } else { Ok(text.to_string()) };
    let channel = |text: &str| ChannelId::parse(text).ok_or_else(|| USAGE.to_string());
    let account = |u: &str, n: &str, act: Act| {
        Ok(Command::As { account: Account { address: address(u)?, identity: name(n)? }, act })
    };
    match words.as_slice() {
        ["register", u, n] => account(u, n, Act::Register(None)),
        ["register", u, n, d] => account(u, n, Act::Register(Some(name(d)?))),
        ["sign-in", u, n] => account(u, n, Act::SignIn),
        ["channels", u, n] => account(u, n, Act::Channels),
        ["speak", u, n, c] => account(u, n, Act::Speak { channel_id: channel(c)?, tone: false }),
        ["speak", u, n, c, "--tone"] => account(u, n, Act::Speak { channel_id: channel(c)?, tone: true }),
        ["listen", u, n, c] => account(u, n, Act::Listen(channel(c)?)),
        ["devices"] => Ok(Command::Devices),
        ["use-microphone", "--default"] => Ok(Command::UseMicrophone(None)),
        ["use-microphone", n] => Ok(Command::UseMicrophone(Some(name(n)?))),
        ["use-speaker", "--default"] => Ok(Command::UseSpeaker(None)),
        ["use-speaker", n] => Ok(Command::UseSpeaker(Some(name(n)?))),
        ["pick"] => Ok(Command::Pick(share::Want::Either)),
        ["pick", "monitor"] => Ok(Command::Pick(share::Want::Monitor)),
        ["pick", "window"] => Ok(Command::Pick(share::Want::Window)),
        ["capture"] => Ok(Command::Capture(share::Want::Either)),
        ["capture", "monitor"] => Ok(Command::Capture(share::Want::Monitor)),
        ["capture", "window"] => Ok(Command::Capture(share::Want::Window)),
        ["share", rest @ ..] => {
            let (want, sound, [u, n, c]) = share_words(rest)?;
            account(u, n, Act::Share { want, sound, channel_id: channel(c)? })
        }
        ["watch", u, n, c] => account(u, n, Act::Watch { channel_id: channel(c)?, to: WatchTo::Reports }),
        ["watch", u, n, c, "--stream", f] => {
            account(u, n, Act::Watch { channel_id: channel(c)?, to: WatchTo::Stream(name(f)?) })
        }
        ["sign-out", u, n] => account(u, n, Act::SignOut),
        ["change-password", u, n] => account(u, n, Act::ChangePassword),
        ["invite", "create", u, n, flags @ ..] => account(u, n, Act::InviteCreate(invite_terms(flags)?)),
        ["invite", "list", u, n] => account(u, n, Act::InviteList),
        ["invite", "revoke", u, n, i] => account(u, n, Act::InviteRevoke(name(i)?)),
        ["invite", "check", u] => Ok(Command::InviteCheck(address(u)?)),
        ["member", "reset", u, n, m] => account(u, n, Act::MemberReset(name(m)?)),
        ["member", "list", u, n] => account(u, n, Act::MemberList),
        ["member", "role", u, n, m, r, way @ ("give" | "take")] => {
            account(u, n, Act::MemberRole { user_id: name(m)?, role: name(r)?, held: *way == "give" })
        }
        ["member", "kick", u, n, m] => account(u, n, Act::MemberKick(name(m)?)),
        ["member", way @ ("ban" | "unban"), u, n, m] => {
            account(u, n, Act::MemberBan { user_id: name(m)?, banned: *way == "ban" })
        }
        ["role", "list", u, n] => account(u, n, Act::RoleList),
        ["role", "create", u, n, r] => account(u, n, Act::RoleCreate { name: name(r)?, colour: None }),
        ["role", "create", u, n, r, c] => account(u, n, Act::RoleCreate { name: name(r)?, colour: Some(colour(c)?) }),
        ["role", "rename", u, n, r, to] => account(u, n, Act::RoleRename { role: name(r)?, name: name(to)? }),
        ["role", "colour", u, n, r] => account(u, n, Act::RoleColour { role: name(r)?, colour: None }),
        ["role", "colour", u, n, r, c] => account(u, n, Act::RoleColour { role: name(r)?, colour: Some(colour(c)?) }),
        ["role", "move", u, n, r] => account(u, n, Act::RoleMove { role: name(r)?, before: None }),
        ["role", "move", u, n, r, b] => account(u, n, Act::RoleMove { role: name(r)?, before: Some(name(b)?) }),
        ["role", "delete", u, n, r] => account(u, n, Act::RoleDelete(name(r)?)),
        ["role", "word", u, n, r, w, way @ ("give" | "take")] => {
            account(u, n, Act::RoleWord { role: name(r)?, word: grant(w)?, held: *way == "give" })
        }
        ["owner", "transfer", u, n, m] => account(u, n, Act::OwnerTransfer(name(m)?)),
        ["leave", u, n] => account(u, n, Act::Leave),
        ["channel", "create", u, n, kind @ ("voice" | "text"), c, rest @ ..] => {
            account(u, n, channel_create(*kind == "voice", name(c)?, rest)?)
        }
        ["channel", "delete", u, n, c] => account(u, n, Act::ChannelDelete(name(c)?)),
        ["channel", "move", u, n, c, rest @ ..] => {
            let [category, before] = flags(rest, ["--category", "--before"])?;
            let category = category.ok_or_else(|| USAGE.to_string())?;
            account(u, n, Act::ChannelMove { channel_id: name(c)?, category, before })
        }
        ["channel", "overwrites", u, n, c] => account(u, n, Act::ChannelOverwrites(name(c)?)),
        ["channel", "overwrite", u, n, c, kind, who, w, state] => {
            account(u, n, channel_overwrite(name(c)?, kind, name(who)?, w, state)?)
        }
        ["category", "create", u, n, k] => account(u, n, Act::CategoryCreate(name(k)?)),
        ["category", "rename", u, n, k, to] => {
            account(u, n, Act::CategoryRename { category_id: name(k)?, name: name(to)? })
        }
        ["category", "delete", u, n, k] => account(u, n, Act::CategoryDelete(name(k)?)),
        ["category", "move", u, n, k] => account(u, n, Act::CategoryMove { category_id: name(k)?, before: None }),
        ["category", "move", u, n, k, b] => {
            account(u, n, Act::CategoryMove { category_id: name(k)?, before: Some(name(b)?) })
        }
        ["server", "picture", u, n, "--none"] => account(u, n, Act::ServerPicture(None)),
        ["server", "picture", u, n, f] => account(u, n, Act::ServerPicture(Some(name(f)?))),
        _ => Err(USAGE.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "ada";

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn invite_and_channel_commands_parse_and_a_bad_flag_is_the_usage() {
        assert!(matches!(
            parse(&args("invite create example.invalid ada")),
            Ok(Command::As { act: Act::InviteCreate(InviteTerms { uses: 1, days: 7, role: None, note: None }), .. })
        ));
        let Ok(Command::As { act: Act::InviteCreate(terms), .. }) =
            parse(&args("invite create example.invalid ada --note for-bo --uses 5 --role moderators --days 30"))
        else {
            panic!("not an invite")
        };
        assert_eq!(
            terms,
            InviteTerms { uses: 5, days: 30, role: Some("moderators".into()), note: Some("for-bo".into()) }
        );
        for bad in [
            "invite create example.invalid ada --uses",
            "invite create example.invalid ada --uses five",
            "invite create example.invalid ada --uses 1 --uses 2",
            "invite create example.invalid ada --colour red",
            "channel create example.invalid ada discussion lobby",
        ] {
            assert_eq!(parse(&args(bad)), Err(USAGE.to_string()), "{bad}");
        }
        assert!(matches!(
            parse(&args("invite list example.invalid ada")),
            Ok(Command::As { act: Act::InviteList, .. })
        ));
        assert!(matches!(
            parse(&args("invite revoke example.invalid ada 3")),
            Ok(Command::As { act: Act::InviteRevoke(_), .. })
        ));
        assert!(matches!(
            parse(&args("channel create example.invalid ada voice booth --category 9")),
            Ok(Command::As { act: Act::ChannelCreate { voice: true, .. }, .. })
        ));
        assert!(
            parse(&args("channel create example.invalid ada voice booth")).is_err(),
            "a channel is made in a category"
        );
        assert!(matches!(
            parse(&args("channel delete example.invalid ada 3")),
            Ok(Command::As { act: Act::ChannelDelete(_), .. })
        ));
        assert!(matches!(
            parse(&args("server picture example.invalid ada owl.png")),
            Ok(Command::As { act: Act::ServerPicture(Some(_)), .. })
        ));
        assert!(matches!(
            parse(&args("server picture example.invalid ada --none")),
            Ok(Command::As { act: Act::ServerPicture(None), .. })
        ));
    }

    #[test]
    fn a_channel_is_made_where_and_as_open_as_asked() {
        let act = |text: &str| match parse(&args(text)) {
            Ok(Command::As { act, .. }) => Ok(act),
            Ok(other) => panic!("{other:?}"),
            Err(said) => Err(said),
        };
        assert_eq!(
            act("channel create example.invalid ada text news --category 9"),
            Ok(Act::ChannelCreate { voice: false, name: "news".into(), category: "9".into(), access: None })
        );
        assert_eq!(
            act("channel create example.invalid ada text news --role stewards --access read-only --category 9"),
            Ok(Act::ChannelCreate {
                voice: false,
                name: "news".into(),
                category: "9".into(),
                access: Some(Access { kept: ChannelAccess::ReadOnly, role: Some("stewards".into()) }),
            })
        );
        assert_eq!(
            act("channel create example.invalid ada voice backroom --access private --category 9"),
            Ok(Act::ChannelCreate {
                voice: true,
                name: "backroom".into(),
                category: "9".into(),
                access: Some(Access { kept: ChannelAccess::Private, role: None }),
            })
        );
        assert_eq!(
            act("channel create example.invalid ada voice booth --access read-only --category 9"),
            Err("A voice channel cannot be read-only.".into())
        );
        for bad in [
            "channel create example.invalid ada text news",
            "channel create example.invalid ada text news --access private",
            "channel create example.invalid ada text news --category 9 --role stewards",
            "channel create example.invalid ada text news --category 9 --access hidden",
            "channel create example.invalid ada text news --category 9 --access private --access read-only",
            "channel create example.invalid ada text news --category 9 --access",
            "channel create example.invalid ada text news --category 9 --before 4",
        ] {
            assert_eq!(parse(&args(bad)), Err(USAGE.to_string()), "{bad}");
        }
    }

    #[test]
    fn an_invite_check_a_reset_and_a_leave_parse() {
        assert!(matches!(parse(&args("invite check example.invalid")), Ok(Command::InviteCheck(_))));
        assert!(matches!(
            parse(&args("member reset example.invalid ada 7")),
            Ok(Command::As { act: Act::MemberReset(ref user_id), .. }) if user_id == "7"
        ));
        assert!(matches!(parse(&args("leave example.invalid ada")), Ok(Command::As { act: Act::Leave, .. })));
        for bad in ["invite check", "member reset example.invalid ada", "leave example.invalid"] {
            assert_eq!(parse(&args(bad)), Err(USAGE.to_string()), "{bad}");
        }
    }

    #[test]
    fn member_role_and_owner_commands_parse() {
        assert!(matches!(
            parse(&args("member list example.invalid ada")),
            Ok(Command::As { act: Act::MemberList, .. })
        ));
        assert!(matches!(parse(&args("role list example.invalid ada")), Ok(Command::As { act: Act::RoleList, .. })));
        assert_eq!(
            parse(&args("member role example.invalid ada 7 admin give")),
            Ok(Command::As {
                account: Account {
                    address: Address::typed("example.invalid").expect("an address"),
                    identity: ADA.into()
                },
                act: Act::MemberRole { user_id: "7".into(), role: "admin".into(), held: true }
            })
        );
        assert!(matches!(
            parse(&args("member role example.invalid ada 7 admin take")),
            Ok(Command::As { act: Act::MemberRole { held: false, .. }, .. })
        ));
        assert!(matches!(
            parse(&args("owner transfer example.invalid ada 7")),
            Ok(Command::As { act: Act::OwnerTransfer(ref user_id), .. }) if user_id == "7"
        ));
        for bad in [
            "member role example.invalid ada 7 admin",
            "member role example.invalid ada 7 admin grant",
            "owner transfer example.invalid ada",
        ] {
            assert_eq!(parse(&args(bad)), Err(USAGE.to_string()), "{bad}");
        }
    }

    #[test]
    fn the_role_ban_overwrite_and_order_commands_parse_their_words_at_the_edge() {
        let act = |text: &str| match parse(&args(text)) {
            Ok(Command::As { act, .. }) => Ok(act),
            Ok(other) => panic!("{other:?}"),
            Err(said) => Err(said),
        };
        let teal = Colour::parse("#2A9D8F").ok();
        assert_eq!(
            act("role create example.invalid ada stewards 2a9d8f"),
            Ok(Act::RoleCreate { name: "stewards".into(), colour: teal })
        );
        assert_eq!(
            act("role colour example.invalid ada stewards"),
            Ok(Act::RoleColour { role: "stewards".into(), colour: None })
        );
        assert_eq!(
            act("role colour example.invalid ada stewards teal"),
            Err("Use six hex digits, like #FF8A5B.".into())
        );
        assert_eq!(
            act("role word example.invalid ada stewards manage_messages give"),
            Ok(Act::RoleWord { role: "stewards".into(), word: Grant::ManageMessages, held: true })
        );
        assert_eq!(act("role word example.invalid ada stewards FLY give"), Err("No word is called FLY.".into()));
        assert_eq!(
            act("role move example.invalid ada stewards"),
            Ok(Act::RoleMove { role: "stewards".into(), before: None })
        );
        assert_eq!(act("member ban example.invalid ada 7"), Ok(Act::MemberBan { user_id: "7".into(), banned: true }));
        assert_eq!(
            act("member unban example.invalid ada 7"),
            Ok(Act::MemberBan { user_id: "7".into(), banned: false })
        );
        assert_eq!(act("member kick example.invalid ada 7"), Ok(Act::MemberKick("7".into())));
        assert_eq!(
            act("channel overwrite example.invalid ada 3 role member SEND_MESSAGE inherit"),
            Ok(Act::ChannelOverwrite {
                channel_id: "3".into(),
                whom: Whom::Role("member".into()),
                word: Grant::SendMessage,
                overwrite: None
            })
        );
        assert_eq!(
            act("channel overwrite example.invalid ada 3 member 7 send_message deny"),
            Ok(Act::ChannelOverwrite {
                channel_id: "3".into(),
                whom: Whom::Member("7".into()),
                word: Grant::SendMessage,
                overwrite: Some(Overwrite::Deny)
            })
        );
        assert_eq!(
            act("channel overwrite example.invalid ada 3 role member MANAGE_ROLES allow"),
            Err("A channel does not take MANAGE_ROLES.".into())
        );
        assert_eq!(
            act("channel move example.invalid ada 3 --before 4 --category 9"),
            Ok(Act::ChannelMove { channel_id: "3".into(), category: "9".into(), before: Some("4".into()) })
        );
        assert_eq!(
            act("channel move example.invalid ada 3 --category 9"),
            Ok(Act::ChannelMove { channel_id: "3".into(), category: "9".into(), before: None })
        );
        assert_eq!(
            act("category move example.invalid ada 9 8"),
            Ok(Act::CategoryMove { category_id: "9".into(), before: Some("8".into()) })
        );
        for bad in [
            "channel move example.invalid ada 3 --before 4 --before 5",
            "channel move example.invalid ada 3",
            "channel move example.invalid ada 3 --before 4",
            "channel move example.invalid ada 3 --category",
            "channel overwrite example.invalid ada 3 role member SEND_MESSAGE maybe",
            "channel overwrite example.invalid ada 3 member SEND_MESSAGE deny",
            "channel overwrite example.invalid ada 3 everyone 7 SEND_MESSAGE deny",
            "role word example.invalid ada stewards SEND_MESSAGE grant",
            "member ban example.invalid ada",
        ] {
            assert_eq!(parse(&args(bad)), Err(USAGE.to_string()), "{bad}");
        }
    }

    #[test]
    fn each_command_parses_with_its_arguments() {
        assert!(matches!(parse(&args("sign-in example.invalid ada")), Ok(Command::As { act: Act::SignIn, .. })));
        assert!(matches!(
            parse(&args("speak example.invalid ada 7")),
            Ok(Command::As { act: Act::Speak { tone: false, .. }, .. })
        ));
        assert!(matches!(
            parse(&args("speak example.invalid ada 7 --tone")),
            Ok(Command::As { act: Act::Speak { tone: true, .. }, .. })
        ));
        assert!(matches!(parse(&args("listen localhost ada 7")), Ok(Command::As { act: Act::Listen(_), .. })));
        assert!(matches!(parse(&args("pick window")), Ok(Command::Pick(share::Want::Window))));
        assert!(matches!(parse(&args("capture monitor")), Ok(Command::Capture(share::Want::Monitor))));
        assert!(matches!(
            parse(&args("share example.invalid ada 7")),
            Ok(Command::As { act: Act::Share { want: share::Want::Either, .. }, .. })
        ));
        assert!(matches!(
            parse(&args("share window example.invalid ada 7")),
            Ok(Command::As { act: Act::Share { want: share::Want::Window, .. }, .. })
        ));
        let Ok(Command::As { act: Act::Share { want, sound, .. }, .. }) =
            parse(&args("share monitor --sound player --sound game example.invalid ada 7"))
        else {
            panic!("a share with its sound")
        };
        assert_eq!((want, sound), (share::Want::Monitor, vec!["player".to_string(), "game".to_string()]));
        for wrong in [
            "share --sound example.invalid ada 7",
            "share --sound player --sound player example.invalid ada 7",
            "share example.invalid ada 7 --sound player",
            "share --sound player monitor example.invalid ada 7",
        ] {
            assert!(parse(&args(wrong)).is_err(), "{wrong}");
        }
        assert!(matches!(
            parse(&args("watch example.invalid ada 7")),
            Ok(Command::As { act: Act::Watch { to: WatchTo::Reports, .. }, .. })
        ));
        assert!(parse(&args("watch example.invalid ada 7 out.y4m")).is_err(), "a file is for --stream alone");
        assert!(matches!(
            parse(&args("watch example.invalid ada 7 --stream out.h264")),
            Ok(Command::As { act: Act::Watch { to: WatchTo::Stream(_), .. }, .. })
        ));
    }

    #[test]
    fn the_device_commands_parse() {
        assert_eq!(parse(&args("devices")), Ok(Command::Devices));
        assert_eq!(parse(&args("use-microphone --default")), Ok(Command::UseMicrophone(None)));
        assert_eq!(
            parse(&["use-speaker".to_string(), "Desk speakers".to_string()]),
            Ok(Command::UseSpeaker(Some("Desk speakers".into())))
        );
        assert_eq!(parse(&args("join-muted on")), Err(USAGE.to_string()), "gone with the setting");
        assert_eq!(parse(&["use-microphone".to_string(), String::new()]), Err(USAGE.to_string()));
    }

    #[test]
    fn a_wrong_count_or_an_unknown_word_is_the_usage_line() {
        for bad in ["", "sign-in", "sign-in example.invalid", "dance example.invalid ada", "register example.invalid"] {
            assert_eq!(parse(&args(bad)), Err(USAGE.to_string()), "{bad}");
        }
    }

    #[test]
    fn a_display_name_at_registration_is_optional() {
        assert!(matches!(
            parse(&args("register example.invalid ada")),
            Ok(Command::As { act: Act::Register(None), .. })
        ));
        assert!(matches!(
            parse(&args("register example.invalid ada Ada")),
            Ok(Command::As { act: Act::Register(Some(_)), .. })
        ));
    }

    #[test]
    fn the_session_endings_parse_too() {
        assert!(matches!(parse(&args("sign-out example.invalid ada")), Ok(Command::As { act: Act::SignOut, .. })));
        assert!(
            matches!(parse(&args("sign-out 127.0.0.1:8443 ada")), Ok(Command::As { act: Act::SignOut, .. })),
            "a bare host:port of this machine is also an address"
        );
        assert!(matches!(
            parse(&args("change-password 127.0.0.1:8443 ada")),
            Ok(Command::As { act: Act::ChangePassword, .. })
        ));
    }

    #[test]
    fn a_bare_name_and_a_written_address_are_both_understood() {
        assert_eq!(
            parse(&args("sign-in example.invalid ada")),
            Ok(Command::As {
                account: Account {
                    address: Address::typed("example.invalid").expect("worth trying"),
                    identity: ADA.to_string()
                },
                act: Act::SignIn
            })
        );
        assert_eq!(
            parse(&args("sign-in wss://example.invalid/v1 ada")),
            Ok(Command::As {
                account: Account {
                    address: Address::typed("wss://example.invalid/v1").expect("a socket named in full"),
                    identity: ADA.to_string()
                },
                act: Act::SignIn
            })
        );
    }

    #[test]
    fn a_bad_address_says_what_is_wrong_with_it() {
        // A plain address to another machine is refused where it is parsed, in words a person can act on.
        let error = parse(&args("sign-in ws://example.invalid ada")).unwrap_err();
        assert!(error.contains("encrypted"), "{error}");
        assert!(!error.contains("://"), "nothing a person reads shows a scheme: {error}");
        assert!(parse(&args("sign-in example.invalid:84xx ada")).is_err(), "a port that is not a port is refused too");
    }
}

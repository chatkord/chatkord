//! The session token kept in the operating system's keyring, one per account on one server.
//! A keyring that is missing or locked is not fatal: the person signs in with the password
//! each time, and is told. One that was not ready when the run began, or that stopped answering,
//! is made again by the next call, so a keyring that comes up later is used from then on. The store is made on a
//! thread of its own, which a call waits for within a deadline: a system's service that never answers costs a call
//! that long once, and every later call answers [`Unavailable`] at once until the thread returns.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use keyring_core::{CredentialStore, Entry, Error};

use crate::Unavailable;
use terms::{Secret, ServerUrl};

/// The service every entry is filed under in the system's keyring.
pub const SERVICE: &str = channel::THIS.id;

/// Makes a credential store, or says why it could not.
pub type Make = Box<dyn Fn() -> keyring_core::Result<Arc<CredentialStore>> + Send + Sync>;

/// How long a call waits for the store to be made before it takes the system's keyring for one that does not answer.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(10);

/// Which call of the keyring something went wrong in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
enum Operation {
    UseSystem,
    Store,
    Load,
    Forget,
}

/// What a failure was, as far as a log may say: the library's variant, never its detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
enum Kind {
    NoDefaultStore,
    NoStorageAccess,
    PlatformFailure,
    NotAnswering,
    Other,
}

impl Kind {
    fn of(error: &Error) -> Self {
        match error {
            Error::NoDefaultStore => Self::NoDefaultStore,
            Error::NoStorageAccess(_) => Self::NoStorageAccess,
            Error::PlatformFailure(_) => Self::PlatformFailure,
            _ => Self::Other,
        }
    }
}

/// Whether the last call that was proved either way worked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Health {
    Working,
    Failing,
}

/// Whether an error means the backend itself is gone or stopped answering, so its store is made again. A locked
/// keyring (`NoStorageAccess`) is the same store waiting for the person, and keeps its store.
fn backend_gone(error: &Error) -> bool {
    matches!(error, Error::PlatformFailure(_) | Error::NoDefaultStore)
}

/// A store being made on its thread, and whether a call already gave up waiting for it.
struct Making {
    answer: Receiver<keyring_core::Result<Arc<CredentialStore>>>,
    given_up: bool,
}

/// The credential store of a run and the calls on it: made when the first call needs it, and again after the
/// backend failed, from the maker it was given, on a thread each call waits for within its deadline.
pub struct Keyring {
    make: OnceLock<Arc<Make>>,
    making: Mutex<Option<Making>>,
    health: Mutex<Health>,
    deadline: Duration,
}

impl Keyring {
    /// A keyring with no maker: it uses the default store when one is set, and answers [`Unavailable`] otherwise. A
    /// call waits [`ANSWER_WITHIN`] for the store.
    pub const fn new() -> Self {
        Self::answering_within(ANSWER_WITHIN)
    }

    /// The same, a call waiting `deadline` for the store.
    pub const fn answering_within(deadline: Duration) -> Self {
        Self { make: OnceLock::new(), making: Mutex::new(None), health: Mutex::new(Health::Working), deadline }
    }

    /// Gives it the maker of its store; a second one is ignored.
    pub fn use_maker(&self, make: Make) {
        let _ = self.make.set(Arc::new(make));
    }

    /// Starts making the store now rather than at the first call, and returns at once.
    pub fn start(&self) {
        let mut making = self.making();
        if making.is_none() && keyring_core::get_default_store().is_none() {
            match self.begin() {
                Ok(begun) => *making = Some(begun),
                Err(error) => {
                    let _ = self.failed(Operation::UseSystem, &error);
                }
            }
        }
    }

    /// The token saved for this account, if one is.
    pub fn load(&self, url: &ServerUrl, identity: &str) -> Result<Option<Secret>, Unavailable> {
        let entry = self.entry(Operation::Load, url, identity)?;
        match entry.get_password() {
            Ok(token) => Ok(self.worked(Some(Secret::new(token)))),
            Err(Error::NoEntry) => Ok(self.worked(None)),
            Err(error) => Err(self.failed(Operation::Load, &error)),
        }
    }

    /// Keeps the token the server issued, filed under that name.
    pub fn store(
        &self,
        url: &ServerUrl,
        looked_up: &str,
        named: Option<String>,
        token: &Secret,
    ) -> Result<(), Unavailable> {
        let entry = self.entry(Operation::Store, url, &filed_under(looked_up, named))?;
        match entry.set_password(token.expose()) {
            Ok(()) => {
                self.worked(());
                Ok(())
            }
            Err(error) => Err(self.failed(Operation::Store, &error)),
        }
    }

    /// Removes the saved token; none saved is not an error.
    pub fn forget(&self, url: &ServerUrl, identity: &str) -> Result<(), Unavailable> {
        let entry = self.entry(Operation::Forget, url, identity)?;
        match entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => {
                self.worked(());
                Ok(())
            }
            Err(error) => Err(self.failed(Operation::Forget, &error)),
        }
    }

    fn entry(&self, operation: Operation, url: &ServerUrl, identity: &str) -> Result<Entry, Unavailable> {
        self.ensure(operation)?;
        Entry::new(SERVICE, &format!("{identity}@{}", url.as_str())).map_err(|error| self.failed(operation, &error))
    }

    /// Begins making the store on a thread of its own, which tells its answer when it has one.
    fn begin(&self) -> Result<Making, Error> {
        let make = Arc::clone(self.make.get().ok_or(Error::NoDefaultStore)?);
        let (told, answer) = mpsc::channel();
        std::thread::Builder::new()
            .name("chatkord-keyring".into())
            .spawn(move || drop(told.send(make())))
            .map_err(|_| Error::NoDefaultStore)?;
        Ok(Making { answer, given_up: false })
    }

    /// The default store, made and set now when there is none. The lock is held while it is waited for, so two calls
    /// make one store; a call that finds the wait given up on answers at once, and takes the store when the thread
    /// has made it since.
    fn ensure(&self, operation: Operation) -> Result<(), Unavailable> {
        let mut slot = self.making();
        if keyring_core::get_default_store().is_some() {
            return Ok(());
        }
        let making = match &mut *slot {
            Some(making) => making,
            empty => empty.insert(self.begin().map_err(|error| self.failed(operation, &error))?),
        };
        let heard = if making.given_up {
            making.answer.try_recv()
        } else {
            making.answer.recv_timeout(self.deadline).map_err(|silent| match silent {
                RecvTimeoutError::Timeout => TryRecvError::Empty,
                RecvTimeoutError::Disconnected => TryRecvError::Disconnected,
            })
        };
        match heard {
            Ok(made) => {
                *slot = None;
                match made {
                    Ok(store) => {
                        keyring_core::set_default_store(store);
                        Ok(())
                    }
                    Err(error) => Err(self.failed(operation, &error)),
                }
            }
            Err(TryRecvError::Empty) => {
                making.given_up = true;
                Err(Self::fall(&mut self.health(), operation, Kind::NotAnswering, false))
            }
            Err(TryRecvError::Disconnected) => {
                *slot = None;
                Err(Self::fall(&mut self.health(), operation, Kind::Other, false))
            }
        }
    }

    fn worked<T>(&self, value: T) -> T {
        *self.health() = Health::Working;
        value
    }

    fn failed(&self, operation: Operation, error: &Error) -> Unavailable {
        Self::fall(&mut self.health(), operation, Kind::of(error), backend_gone(error))
    }

    /// A failure: the store goes when its backend is gone, and a turn from working to failing is noted once, with the
    /// call and the kind of error, never what it says.
    fn fall(health: &mut Health, operation: Operation, kind: Kind, gone: bool) -> Unavailable {
        if gone {
            keyring_core::unset_default_store();
        }
        if std::mem::replace(health, Health::Failing) == Health::Working {
            report::note!(warn, "the keyring failed", operation = operation, kind = kind);
        }
        Unavailable
    }

    fn making(&self) -> MutexGuard<'_, Option<Making>> {
        self.making.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn health(&self) -> MutexGuard<'_, Health> {
        self.health.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for Keyring {
    fn default() -> Self {
        Self::new()
    }
}

/// This run's keyring, which [`use_system`] gives the system's maker.
static SYSTEM: Keyring = Keyring::new();

/// The system's store, as this system's crate makes it.
#[cfg(any(target_os = "linux", windows))]
fn make_system() -> keyring_core::Result<Arc<CredentialStore>> {
    #[cfg(target_os = "linux")]
    let store: Arc<CredentialStore> = zbus_secret_service_keyring_store::Store::new()?;
    #[cfg(windows)]
    let store: Arc<CredentialStore> = windows_native_keyring_store::Store::new()?;
    Ok(store)
}

/// Makes this system's credential store the one every entry uses, at start. Where there is none, or it cannot be
/// opened, every call answers [`Unavailable`] and asks the system again.
pub fn use_system() {
    #[cfg(any(target_os = "linux", windows))]
    {
        SYSTEM.use_maker(Box::new(make_system));
        SYSTEM.start();
    }
}

/// The token saved for this account, if one is.
pub fn load(url: &ServerUrl, identity: &str) -> Result<Option<Secret>, Unavailable> {
    SYSTEM.load(url, identity)
}

/// The name a token, and the account a run remembers, is filed under: the identity the server named, since a
/// sign-in folds a name's case, else the name looked up, the only one a server that names none shares.
pub fn filed_under(looked_up: &str, named: Option<String>) -> String {
    named.unwrap_or_else(|| looked_up.to_string())
}

/// Keeps the token the server issued, filed under that name.
pub fn store(url: &ServerUrl, looked_up: &str, named: Option<String>, token: &Secret) -> Result<(), Unavailable> {
    SYSTEM.store(url, looked_up, named, token)
}

/// Removes the saved token; none saved is not an error.
pub fn forget(url: &ServerUrl, identity: &str) -> Result<(), Unavailable> {
    SYSTEM.forget(url, identity)
}

/// The token saved for this account, if one is.
pub async fn load_token(url: &ServerUrl, identity: &str) -> Result<Option<Secret>, Unavailable> {
    let (url, identity) = (url.clone(), identity.to_string());
    crate::blocking(move || load(&url, &identity)).await
}

/// Keeps the token the server issued, and remembers the account at that server whichever way the keyring went.
pub async fn store_token(
    platform: &crate::Platform,
    url: &ServerUrl,
    looked_up: &str,
    named: Option<String>,
    token: Secret,
) -> Result<(), Unavailable> {
    let filed = filed_under(looked_up, named.clone());
    let _ = crate::state::remember_account(platform, url.clone(), filed).await;
    let (url, looked_up) = (url.clone(), looked_up.to_string());
    crate::blocking(move || store(&url, &looked_up, named, &token)).await
}

/// Removes the saved token; none saved is not an error.
pub async fn forget_token(url: &ServerUrl, identity: &str) -> Result<(), Unavailable> {
    let (url, identity) = (url.clone(), identity.to_string());
    crate::blocking(move || forget(&url, &identity)).await
}

#[cfg(test)]
mod tests {
    use super::filed_under;

    /// The name the store made, and the same name as a person reading a display name typed it.
    const NAMED: &str = "casing";
    const TYPED: &str = "CASING";

    #[test]
    fn a_token_is_filed_under_the_name_the_server_named_and_not_the_one_that_was_typed() {
        assert_eq!(filed_under(TYPED, Some(NAMED.into())).as_str(), NAMED);
    }

    #[test]
    fn a_server_that_names_no_identity_leaves_the_name_that_was_looked_up() {
        assert_eq!(filed_under(NAMED, None).as_str(), NAMED);
    }
}

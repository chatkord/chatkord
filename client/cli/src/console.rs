//! The command line run from a real terminal: its arguments read, the command carried out, its sentence said.

use crate::command::USAGE;
use crate::{Ask, Terminal, parse, run};
use link::Trust;
use std::ffi::OsString;
use std::future::Future;
use std::pin::Pin;
use std::process::ExitCode;
use terms::Secret;

/// The real terminal. `CHATKORD_PASSWORD`, `CHATKORD_NEW_PASSWORD` and `CHATKORD_INVITE` stand in
/// for the prompts so a script can run it; a person is asked, without echo.
struct Console;

impl Terminal for Console {
    fn secret(&mut self, ask: Ask) -> Pin<Box<dyn Future<Output = Result<Secret, String>> + Send>> {
        let (variable, prompt) = match ask {
            Ask::Password => ("CHATKORD_PASSWORD", "Password: "),
            Ask::PasswordAgain => ("CHATKORD_PASSWORD", "Password again: "),
            Ask::NewPassword => ("CHATKORD_NEW_PASSWORD", "New password: "),
            Ask::NewPasswordAgain => ("CHATKORD_NEW_PASSWORD", "New password again: "),
            Ask::Invite => ("CHATKORD_INVITE", "Invite code: "),
        };
        let scripted = std::env::var(variable).ok();
        Box::pin(async move {
            let value = match scripted {
                Some(value) => value,
                None => tokio::task::spawn_blocking(move || rpassword::prompt_password(prompt))
                    .await
                    .map_err(|_| "Could not read from the terminal.".to_string())?
                    .map_err(|_| "Could not read from the terminal.".to_string())?,
            };
            Ok(Secret::new(value))
        })
    }

    #[expect(clippy::print_stdout, reason = "the terminal is this front end's screen")]
    fn say(&mut self, line: &str) {
        println!("{line}");
    }

    fn stop(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async {
            let _ = tokio::signal::ctrl_c().await;
        })
    }
}

/// Runs the command `args` name (the program's own name not among them) on a runtime of its own, and says why it was
/// refused on stderr: an argument that is not UTF-8 names no command, and is refused with the usage line.
#[expect(clippy::print_stderr, reason = "a refused command says why on the terminal")]
pub fn main(args: impl Iterator<Item = OsString>) -> ExitCode {
    match command_line(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(sentence) => {
            eprintln!("{sentence}");
            ExitCode::FAILURE
        }
    }
}

fn command_line(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let args = args.map(|arg| arg.into_string().map_err(|_| USAGE.to_string())).collect::<Result<Vec<_>, _>>()?;
    let command = parse(&args)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "The command line could not start.".to_string())?;
    platform::keyring::use_system();
    let platform = platform::Platform::system();
    runtime.block_on(run(command, &mut Console, &platform, &Trust::system()))
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn not_utf8() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xff, 0xfe])
    }

    #[cfg(windows)]
    fn not_utf8() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        // A lone surrogate, which UTF-8 cannot carry.
        OsString::from_wide(&[0xd800])
    }

    #[test]
    fn an_argument_that_is_not_utf8_is_refused_with_the_usage_line() {
        let args = || [OsString::from("channels"), not_utf8(), OsString::from("ada")];
        assert_eq!(command_line(args().into_iter()), Err(USAGE.to_string()));
        assert_eq!(main(args().into_iter()), ExitCode::FAILURE);
    }
}

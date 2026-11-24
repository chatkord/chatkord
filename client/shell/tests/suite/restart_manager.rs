//! An installer's Restart Manager closes a running window as a person's close does, so the window parts before it ends.

mod lab {
    use crate::common::{Home, Which, exited, own_copy, shown, start_from};
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::RestartManager::{
        CCH_RM_SESSION_KEY, RmEndSession, RmRegisterResources, RmShutdown, RmStartSession,
    };
    use windows_sys::core::PCWSTR;

    #[test]
    fn restart_manager_closes_the_window_and_it_exits_cleanly() {
        closed_once_shown(Which::Main);
    }

    /// A close as the program starts reaches winit's own window alone, before ours are made.
    #[test]
    fn restart_manager_closes_a_window_just_started_and_it_exits_cleanly() {
        closed_once_shown(Which::First);
    }

    /// Starts the window, closes it as an installer does once `which` shows, and checks it ended cleanly.
    fn closed_once_shown(which: Which) {
        tier::lab();
        let home = Home::new("rm");
        let exe = own_copy(&home);
        let mut window = start_from(&exe, &home);
        let shown = shown(&mut window, which).is_some();

        let shut = shut_down(&exe.to_string_lossy());
        let ended = exited(&mut window);
        if ended.is_none() {
            let _ = window.kill();
        }

        assert!(shown, "the window never showed");
        assert_eq!(shut, ERROR_SUCCESS, "Restart Manager could not close the window");
        let status = ended.expect("the window was still running after Restart Manager closed it");
        assert!(status.success(), "the window exited with {status}");
    }

    /// Asks Restart Manager, as an installer does, to close whatever holds `exe`, without forcing it.
    fn shut_down(exe: &str) -> u32 {
        let mut session = 0;
        let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
        // SAFETY: `session` and `key` are valid out pointers, `key` of the size the call asks.
        let started = unsafe { RmStartSession(&raw mut session, 0, key.as_mut_ptr()) };
        assert_eq!(started, ERROR_SUCCESS, "no Restart Manager session");
        let path: Vec<u16> = exe.encode_utf16().chain([0]).collect();
        let files: [PCWSTR; 1] = [path.as_ptr()];
        // SAFETY: `files` names one NUL-terminated path, alive for the call.
        let registered =
            unsafe { RmRegisterResources(session, 1, files.as_ptr(), 0, std::ptr::null(), 0, std::ptr::null()) };
        assert_eq!(registered, ERROR_SUCCESS, "the executable could not be registered");
        // SAFETY: an open session, no callback.
        let shut = unsafe { RmShutdown(session, 0, None) };
        // SAFETY: the session opened above.
        unsafe { RmEndSession(session) };
        shut
    }
}

//! The CPU time this process has spent, which a benchmark and a measuring test read alike.

/// The CPU time this process has spent, every thread's, in seconds; none where the system does not say.
pub(crate) fn cpu_seconds() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        // `/proc/self/stat`: utime and stime are the 12th and 13th fields after the command's closing parenthesis, in
        // the kernel's clock ticks, which are 100 a second on every Linux this runs on.
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        let mut after = stat.rsplit_once(')')?.1.split_whitespace().skip(11);
        let ticks = after.next()?.parse::<u64>().ok()? + after.next()?.parse::<u64>().ok()?;
        Some(ticks as f64 / 100.0)
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let (mut created, mut ended, mut kernel, mut user) = Default::default();
        // SAFETY: takes nothing; the pseudo-handle needs no closing.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: the current process's pseudo-handle, and four live out-pointers.
        unsafe { GetProcessTimes(process, &raw mut created, &raw mut ended, &raw mut kernel, &raw mut user) }.ok()?;
        // FILETIME counts 100 ns intervals.
        let seconds =
            |time: FILETIME| (u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)) as f64 / 1e7;
        Some(seconds(kernel) + seconds(user))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    None
}

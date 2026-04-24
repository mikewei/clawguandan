#[cfg(unix)]
pub(crate) fn signal_terminate(pid: u32) -> Result<(), String> {
    unix_signal_process(pid, libc::SIGTERM)
}

#[cfg(unix)]
pub(crate) fn signal_force_kill(pid: u32) -> Result<(), String> {
    unix_signal_process(pid, libc::SIGKILL)
}

#[cfg(unix)]
pub(crate) fn is_process_exited(pid: u32) -> bool {
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return false;
    }
    matches!(last_os_error_code(), Some(code) if code == libc::ESRCH)
}

#[cfg(unix)]
pub(crate) fn last_os_error_code() -> Option<i32> {
    std::io::Error::last_os_error().raw_os_error()
}

#[cfg(unix)]
fn unix_signal_process(pid: u32, sig: i32) -> Result<(), String> {
    let rc = unsafe { libc::kill(pid as libc::pid_t, sig) };
    if rc == 0 {
        return Ok(());
    }

    let errno = last_os_error_code().unwrap_or(0);
    if errno == libc::ESRCH {
        return Ok(());
    }
    Err(std::io::Error::from_raw_os_error(errno).to_string())
}

#[cfg(not(unix))]
pub(crate) fn signal_terminate(_pid: u32) -> Result<(), String> {
    Err("server stop is only supported on Unix (use WSL2 on Windows)".into())
}

#[cfg(not(unix))]
pub(crate) fn signal_force_kill(_pid: u32) -> Result<(), String> {
    Err("server stop is only supported on Unix (use WSL2 on Windows)".into())
}

#[cfg(not(unix))]
pub(crate) fn is_process_exited(_pid: u32) -> bool {
    false
}

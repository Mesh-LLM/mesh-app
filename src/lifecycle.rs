//! Only signal a child retained by this process; never stop a daemon by port/name.
use std::process::Child;

pub fn request_stop(child: &mut Child) -> Result<(), String> {
    if child.try_wait().map_err(|e| e.to_string())?.is_some() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        // The retained, unreaped Child prevents PID reuse while signalling.
        let result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    }
    #[cfg(windows)]
    {
        // Windows has no Unix-style signal for this CREATE_NO_WINDOW child.
        // Terminate only the retained process handle, never a listener or PID lookup.
        // This is forced termination, not a graceful engine shutdown.
        child.kill().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sleeper() -> Child {
        #[cfg(unix)]
        let mut command = std::process::Command::new("sleep");
        #[cfg(unix)]
        command.arg("30");
        #[cfg(windows)]
        let mut command = std::process::Command::new("ping.exe");
        #[cfg(windows)]
        command.args(["-n", "31", "127.0.0.1"]);
        command.stdout(std::process::Stdio::null()).spawn().unwrap()
    }

    #[test]
    fn already_exited_child_is_safe_to_stop_again() {
        let mut child = sleeper();
        child.kill().unwrap();
        child.wait().unwrap();
        request_stop(&mut child).unwrap();
    }

    #[test]
    fn stops_and_reaps_only_retained_child() {
        let mut owned = sleeper();
        let mut other = sleeper();
        request_stop(&mut owned).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let stopped = loop {
            if owned.try_wait().unwrap().is_some() {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let other_alive = other.try_wait().unwrap().is_none();
        if !stopped {
            let _ = owned.kill();
        }
        let _ = owned.wait();
        other.kill().unwrap();
        other.wait().unwrap();
        assert!(stopped);
        assert!(other_alive);
    }
}

//! Only signal a child retained by this process; never stop a daemon by port/name.
use std::process::Child;

pub fn request_stop(child: &mut Child, console_port: u16) -> Result<(), String> {
    if child.try_wait().map_err(|e| e.to_string())?.is_some() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let _ = console_port;
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
        crate::status::stop_owned(console_port, child.id())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn stops_and_reaps_only_retained_child() {
        let mut owned = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        request_stop(&mut owned, 0).unwrap();
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

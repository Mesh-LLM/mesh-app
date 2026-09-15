//! Say why the runtime stopped, using its own last words.
//!
//! When startup fails there is no console to open, so the reason has to travel
//! with the error the user is already looking at. The runtime writes one
//! `"level":"fatal"` line before exiting; that line is the explanation. If there
//! is no fatal line we fall back to the last thing written, which is better than
//! sending the user to a log they then have to read themselves.
use std::io::Read;
use std::path::Path;

/// Bytes of log read from the end. Enough for a fatal line and its neighbours,
/// small enough that a multi-megabyte log costs nothing to explain.
const TAIL: u64 = 64 * 1024;

/// The runtime's own reason for stopping, if it left one.
///
/// `from` is the log length recorded when this run was started, so a fatal line
/// left by an earlier run is never reported as this run's reason.
pub fn reason(log: &Path, from: u64) -> Option<String> {
    reason_in(&tail(log, from)?)
}

fn tail(log: &Path, from: u64) -> Option<String> {
    use std::io::Seek;
    let mut file = std::fs::File::open(log).ok()?;
    let len = file.metadata().ok()?.len();
    let start = from.min(len).max(len.saturating_sub(TAIL));
    if start > 0 {
        file.seek(std::io::SeekFrom::Start(start)).ok()?;
    }
    let mut text = String::new();
    // Lossy: a partial line at the seek point, or non-UTF8 native output, must
    // not lose the fatal line that follows it.
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    text.push_str(&String::from_utf8_lossy(&bytes));
    Some(text)
}

fn reason_in(text: &str) -> Option<String> {
    let mut last_line = None;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        last_line = Some(line);
        if let Some(message) = fatal_message(line) {
            return Some(message);
        }
    }
    last_line.map(|line| line.trim().to_string())
}

/// Pull the human message out of a fatal JSON log line. Returns `None` for any
/// other line, including errors and warnings the runtime recovered from.
fn fatal_message(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let object = value.as_object()?;
    let field = |name: &str| object.get(name).and_then(|v| v.as_str()) == Some("fatal");
    if !(field("level") || field("event")) {
        return None;
    }
    let message = object
        .get("fatal")
        .or_else(|| object.get("message"))?
        .as_str()?;
    Some(message.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FATAL: &str = r#"{"event":"fatal","level":"fatal","fatal":"Owner identity is required but no keystore was found.","message":"Owner identity is required but no keystore was found."}"#;

    #[test]
    fn reports_the_fatal_line_not_the_last_line() {
        let text = format!("{FATAL}\n{{\"level\":\"info\",\"message\":\"shutting down\"}}\n");
        assert_eq!(
            reason_in(&text).unwrap(),
            "Owner identity is required but no keystore was found."
        );
    }

    #[test]
    fn ignores_errors_the_runtime_survived() {
        let text = "{\"level\":\"error\",\"message\":\"relay not connected\"}\n{\"level\":\"info\",\"message\":\"api ready\"}\n";
        assert_eq!(
            reason_in(text).unwrap(),
            "{\"level\":\"info\",\"message\":\"api ready\"}"
        );
    }

    #[test]
    fn falls_back_to_the_last_line_when_there_is_no_fatal() {
        assert_eq!(
            reason_in("plain crash output\n\n").unwrap(),
            "plain crash output"
        );
    }

    #[test]
    fn nothing_to_say_about_an_empty_log() {
        assert!(reason_in("").is_none());
        assert!(reason_in("   \n\n").is_none());
    }

    #[test]
    fn reads_the_end_of_a_long_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mesh.log");
        let filler = "{\"level\":\"info\",\"message\":\"noise\"}\n".repeat(4000);
        std::fs::write(&path, format!("{filler}{FATAL}\n")).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > TAIL);
        assert_eq!(
            reason(&path, 0).unwrap(),
            "Owner identity is required but no keystore was found."
        );
    }

    #[test]
    fn an_earlier_runs_fatal_is_not_this_runs_reason() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mesh.log");
        let earlier = format!("{FATAL}\n");
        std::fs::write(&path, &earlier).unwrap();
        let mark = earlier.len() as u64;
        assert!(reason(&path, mark).is_none());
        std::fs::write(&path, format!("{earlier}later output\n")).unwrap();
        assert_eq!(reason(&path, mark).unwrap(), "later output");
    }

    #[test]
    fn no_log_is_not_an_error() {
        assert!(reason(Path::new("/nonexistent/mesh.log"), 0).is_none());
    }
}

//! One tray-owned engine default: no visible chain-of-thought in tray chat.
//!
//! This is a launcher policy expressed in the engine's own documented config
//! schema (`docs/USAGE.md`, `[defaults.request_defaults]`), not a second
//! settings product and not an engine change. The file lives in the app-owned
//! subprocess HOME, so the user's real `~/.mesh-llm/config.toml` is untouched.
use std::path::{Path, PathBuf};

/// First line of a tray-written file. Its absence means a human owns the file.
const MARKER: &str = "# Written by the Mesh tray.";

/// Thinking models spend their whole token budget reasoning and often return no
/// answer at all. A tray chat window is the one place that failure is
/// unrecoverable, because the user has no other model to switch to. `off` asks
/// the chat template not to think (`enable_thinking = false`); `hidden` is the
/// belt-and-braces case where a model's embedded template ignores that, so the
/// reasoning at least does not land in the answer.
fn contents() -> String {
    format!(
        "{MARKER}\n\
         # Delete this file, or remove the line above, to manage it yourself.\n\
         # Anything you put here is the engine's documented config schema; run\n\
         # `mesh-llm config validate` against this path to check your edits.\n\
         \n\
         [defaults.request_defaults]\n\
         reasoning_enabled = \"off\"\n\
         reasoning_format = \"hidden\"\n"
    )
}

/// Write the tray's engine defaults into `home`, unless a human owns the file.
///
/// Returns the path when this call wrote it, `None` when an existing
/// user-owned file was deliberately left alone. Never fails the launch for a
/// file it merely declined to replace.
pub fn ensure_defaults(home: &Path) -> Result<Option<PathBuf>, String> {
    let directory = home.join(".mesh-llm");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let path = directory.join("config.toml");
    let wanted = contents();
    match std::fs::read_to_string(&path) {
        Ok(existing) if !existing.starts_with(MARKER) => Ok(None),
        Ok(existing) if existing == wanted => Ok(Some(path)),
        Ok(_) => write(&path, &wanted).map(|()| Some(path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            write(&path, &wanted).map(|()| Some(path))
        }
        Err(e) => Err(e.to_string()),
    }
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_profile_gets_thinking_turned_off() {
        let root = tempfile::tempdir().unwrap();
        let path = ensure_defaults(root.path()).unwrap().unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("[defaults.request_defaults]"));
        assert!(written.contains("reasoning_enabled = \"off\""));
        assert!(written.contains("reasoning_format = \"hidden\""));
        assert_eq!(path, root.path().join(".mesh-llm/config.toml"));
    }

    #[test]
    fn rewriting_is_idempotent_and_refreshes_a_stale_tray_file() {
        let root = tempfile::tempdir().unwrap();
        let path = ensure_defaults(root.path()).unwrap().unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert_eq!(ensure_defaults(root.path()).unwrap(), Some(path.clone()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        // An older tray release's file carries the marker and is refreshed.
        std::fs::write(&path, format!("{MARKER}\n[defaults.request_defaults]\n")).unwrap();
        assert_eq!(ensure_defaults(root.path()).unwrap(), Some(path.clone()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
    }

    #[test]
    fn a_user_owned_config_is_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".mesh-llm/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mine = "[defaults.request_defaults]\nreasoning_enabled = \"on\"\n";
        std::fs::write(&path, mine).unwrap();
        assert_eq!(ensure_defaults(root.path()).unwrap(), None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), mine);
    }
}

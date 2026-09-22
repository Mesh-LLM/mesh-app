//! First-run engine config, then hands the file over for good.
//!
//! The tray runs the engine as the user against their own `~/.mesh-llm`, so
//! `config.toml` is the user's file. On a machine that has never had one, there
//! is nothing to take over and the tray writes a first-run config: thinking off,
//! because a tray chat window is the one place a model that spends its whole
//! token budget reasoning is unrecoverable -- the user has no other model to
//! switch to. After that the file is theirs: it is never rewritten, refreshed
//! or replaced, and their edits stand even if a later release would write
//! something different.
use std::path::{Path, PathBuf};

/// What `ensure_first_run` did, so the launcher can say so in its log.
#[derive(Debug, PartialEq, Eq)]
pub enum Config {
    /// No config existed; this one was written.
    Created(PathBuf),
    /// A config was already there and was left exactly as it was.
    Existing(PathBuf),
}

/// `off` asks the chat template not to think (`enable_thinking = false`);
/// `hidden` is the belt-and-braces case where a model's embedded template
/// ignores that, so reasoning at least does not land in the answer. Both keys
/// are the engine's own documented schema (`docs/USAGE.md`,
/// `[defaults.request_defaults]`), not a second settings product.
fn contents() -> String {
    "# Written by Mesh on first run. It is your file now: edit it freely, and\n\
     # Mesh will not rewrite it. `mesh-llm config validate` checks your edits.\n\
     \n\
     [defaults.request_defaults]\n\
     reasoning_enabled = \"off\"\n\
     reasoning_format = \"hidden\"\n"
        .to_string()
}

/// Write a starting config if this machine has none. Never touches an existing
/// one, whoever wrote it.
pub fn ensure_first_run(profile: &Path) -> Result<Config, String> {
    std::fs::create_dir_all(profile).map_err(|e| e.to_string())?;
    let path = profile.join("config.toml");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Ok(Config::Existing(path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // create_new: if the engine or the user wins the race, they win.
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    file.write_all(contents().as_bytes())
                        .map_err(|e| e.to_string())?;
                    Ok(Config::Created(path))
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    Ok(Config::Existing(path))
                }
                Err(e) => Err(format!("Could not write {}: {e}", path.display())),
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

/// True when the config declares startup models.
///
/// `--model` on the command line beats `[[models]]` in the file, so a tray that
/// always passed the flag would silently ignore a model the user configured.
/// When the file names models, the tray passes no `--model` and the file
/// decides. The tray's first-run config declares none, so this is false until
/// someone adds one.
pub fn config_declares_models(profile: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(profile.join("config.toml")) else {
        return false;
    };
    // A file we cannot parse is the engine's to complain about, not ours to
    // interpret: fall back to the tray's pick rather than starting with none.
    toml::from_str::<toml::Value>(&text)
        .map(|value| value.get("models").is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_turns_thinking_off_and_then_never_touches_the_file_again() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("fresh/.mesh-llm");
        let path = match ensure_first_run(&profile).unwrap() {
            Config::Created(path) => path,
            other => panic!("expected a first-run config, got {other:?}"),
        };
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("[defaults.request_defaults]"));
        assert!(written.contains("reasoning_enabled = \"off\""));
        assert!(written.contains("reasoning_format = \"hidden\""));
        assert_eq!(path, profile.join("config.toml"));

        // Second launch: reported as existing, byte-identical, not refreshed.
        assert_eq!(
            ensure_first_run(&profile).unwrap(),
            Config::Existing(path.clone())
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), written);

        // Their edits stand, including ones that reverse the tray's default.
        let mine = "[defaults.request_defaults]\nreasoning_enabled = \"on\"\n";
        std::fs::write(&path, mine).unwrap();
        assert_eq!(
            ensure_first_run(&profile).unwrap(),
            Config::Existing(path.clone())
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), mine);
    }

    #[test]
    fn an_existing_config_is_never_replaced_even_when_it_is_odd() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path();
        let path = profile.join("config.toml");
        for existing in ["", "not toml = = =", "version = 1\n"] {
            std::fs::write(&path, existing).unwrap();
            assert_eq!(
                ensure_first_run(profile).unwrap(),
                Config::Existing(path.clone())
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), existing);
        }
    }

    #[test]
    fn a_configured_model_is_obeyed_and_the_first_run_file_declares_none() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path();
        let path = profile.join("config.toml");
        // No file at all: nothing to obey, and none created by asking.
        assert!(!config_declares_models(profile));
        assert!(!path.exists());
        // The tray's own first-run file names no models.
        ensure_first_run(profile).unwrap();
        assert!(!config_declares_models(profile));
        // With models: the file decides and the tray adds no --model.
        std::fs::write(&path, "version = 1\n[[models]]\nname = \"mine\"\n").unwrap();
        assert!(config_declares_models(profile));
        // Without models: the tray still supplies its pick.
        std::fs::write(
            &path,
            "version = 1\n[defaults.model_fit]\nctx_size = 4096\n",
        )
        .unwrap();
        assert!(!config_declares_models(profile));
        // Unparsable: the engine reports it; the tray does not start modelless.
        std::fs::write(&path, "this is not toml = = =\n").unwrap();
        assert!(!config_declares_models(profile));
    }
}

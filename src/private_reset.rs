//! Explicit private-state retirement, only after verified engine shutdown.
//! A durable marker prevents startup after partial deletion or failed settings save.
use std::path::{Path, PathBuf};

const STATE: [&str; 4] = [
    "mesh-id",
    "mesh-genesis-policy.json",
    "mesh-adopted-membership.json",
    "last-mesh",
];
const MARKER: &str = "private-reset-pending";

pub fn pending(launcher: &Path) -> bool {
    // A dangling link must also block startup.
    !matches!(std::fs::symlink_metadata(launcher.join(MARKER)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}

pub fn validate_default_profile() -> Result<PathBuf, String> {
    if cfg!(windows) {
        return Err(
            "Reset is not available on Windows until engine liveness can be verified.".into(),
        );
    }
    let profile = crate::settings::mesh_profile()?;
    let key = mesh_llm_identity::default_node_key_path().map_err(|e| e.to_string())?;
    if key != profile.join("key") {
        return Err(
            "Reset is unavailable with a custom node-key path. No private state was changed."
                .into(),
        );
    }
    Ok(profile)
}

/// Refuse remaining tracked engines rather than interfering with another app.
/// Scan the selected root and standard root, because CLI/Buzz can use either.
pub fn ensure_no_runtime() -> Result<(), String> {
    let default = crate::settings::mesh_profile()?.join("runtime");
    check_runtime_locks(&default)?;
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
        check_runtime_locks(&PathBuf::from(xdg).join("mesh-llm/runtime"))?;
    }
    if let Some(root) = std::env::var_os("MESH_LLM_RUNTIME_ROOT") {
        check_runtime_locks(&PathBuf::from(root))?;
    }
    Ok(())
}

fn check_runtime_locks(root: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let lock = entry.path().join("lock");
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock)
        {
            Ok(file) => {
                fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
                    "Another Mesh runtime is active. Quit it before resetting this shared profile."
                        .to_string()
                })?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

pub fn retire(profile: &Path, launcher: &Path) -> Result<(), String> {
    // Validate every entry before mutating anything. Never follow links.
    for name in STATE {
        match std::fs::symlink_metadata(profile.join(name)) {
            Ok(meta) if meta.is_file() => {}
            Ok(_) => return Err(format!("{name} is not a regular file")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if !pending(launcher) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(launcher.join(MARKER))
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        sync_directory(launcher)?;
    }
    for name in STATE {
        match std::fs::remove_file(profile.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if profile.exists() {
        sync_directory(profile)?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    std::fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Call only after launcher defaults have been saved successfully.
pub fn finish(launcher: &Path) -> Result<(), String> {
    sync_directory(launcher)?;
    std::fs::remove_file(launcher.join(MARKER)).map_err(|e| e.to_string())?;
    sync_directory(launcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retires_only_mesh_state_and_can_resume_after_interruption() {
        let profile = tempfile::tempdir().unwrap();
        let launcher = tempfile::tempdir().unwrap();
        let kept = [
            "owner-keystore.json",
            "node-ownership.json",
            "key",
            "wallet.json",
            "config.toml",
            "model.gguf",
        ];
        for name in STATE.into_iter().chain(kept) {
            std::fs::write(profile.path().join(name), name).unwrap();
        }
        retire(profile.path(), launcher.path()).unwrap();
        assert!(pending(launcher.path()));
        for name in STATE {
            assert!(!profile.path().join(name).exists());
        }
        for name in kept {
            assert_eq!(
                std::fs::read(profile.path().join(name)).unwrap(),
                name.as_bytes()
            );
        }
        retire(profile.path(), launcher.path()).unwrap();
        finish(launcher.path()).unwrap();
        assert!(!pending(launcher.path()));
    }
    #[test]
    fn settings_save_failure_keeps_startup_blocked_until_retry() {
        let profile = tempfile::tempdir().unwrap();
        let launcher = tempfile::tempdir().unwrap();
        std::fs::write(profile.path().join("mesh-id"), "old").unwrap();
        retire(profile.path(), launcher.path()).unwrap();
        std::fs::create_dir(launcher.path().join("launcher.json")).unwrap();
        let defaults = crate::settings::Settings::default();
        assert!(defaults.save(launcher.path()).is_err());
        assert!(pending(launcher.path()));
        std::fs::remove_dir(launcher.path().join("launcher.json")).unwrap();
        retire(profile.path(), launcher.path()).unwrap();
        defaults.save(launcher.path()).unwrap();
        finish(launcher.path()).unwrap();
        assert!(!pending(launcher.path()));
        assert_eq!(
            crate::settings::Settings::load(launcher.path())
                .unwrap()
                .connection,
            crate::settings::Connection::Automatic
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_state_never_removes_its_target() {
        let profile = tempfile::tempdir().unwrap();
        let launcher = tempfile::tempdir().unwrap();
        let target = profile.path().join("wallet.json");
        std::fs::write(&target, "preserve").unwrap();
        std::os::unix::fs::symlink(&target, profile.path().join("mesh-id")).unwrap();
        assert!(retire(profile.path(), launcher.path()).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "preserve");
        assert!(!pending(launcher.path()));
    }

    #[test]
    fn invalid_entry_leaves_everything_untouched() {
        let profile = tempfile::tempdir().unwrap();
        let launcher = tempfile::tempdir().unwrap();
        std::fs::write(profile.path().join("mesh-id"), "old").unwrap();
        std::fs::create_dir(profile.path().join("last-mesh")).unwrap();
        assert!(retire(profile.path(), launcher.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(profile.path().join("mesh-id")).unwrap(),
            "old"
        );
        assert!(!pending(launcher.path()));
    }
    #[test]
    fn live_runtime_lock_blocks_reset() {
        let root = tempfile::tempdir().unwrap();
        let instance = root.path().join("123");
        std::fs::create_dir(&instance).unwrap();
        let file = std::fs::File::create(instance.join("lock")).unwrap();
        fs2::FileExt::lock_exclusive(&file).unwrap();
        assert!(check_runtime_locks(root.path()).is_err());
        drop(file);
        assert!(check_runtime_locks(root.path()).is_ok());
    }
}

//! Subprocess-only HOME isolation for the official runtime (no --profile-dir).
//! The GUI retains OS HOME. On macOS only the OS-selected default keychain is
//! linked, not the personal Mesh directory or the entire Library tree. Keychain
//! ACLs still apply; no credential is read, copied, exported or rewritten here.
use std::path::Path;
#[cfg(any(target_os = "macos", test))]
use std::path::PathBuf;

pub fn prepare(home: &Path) -> Result<(), String> {
    std::fs::create_dir_all(home.join(".mesh-llm")).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/security")
            .args(["default-keychain", "-d", "user"])
            .output()
            .map_err(|e| format!("Cannot locate OS keychain: {e}"))?;
        if !output.status.success() {
            return Err("Unlock your OS default keychain and retry.".into());
        }
        let path = parse_keychain(&String::from_utf8_lossy(&output.stdout))?;
        let directory = home.join("Library/Keychains");
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        // Security.framework's synthetic-HOME lookup uses this conventional name.
        // Resolve its target from the OS, never guess the user's keychain path.
        ensure_link(&directory.join("login.keychain-db"), &path)?;
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn parse_keychain(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    let path = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .ok_or("OS returned an unrecognized default keychain path")?;
    let path = PathBuf::from(path);
    if !path.is_absolute()
        || path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
    {
        return Err("OS default keychain path must be absolute".into());
    }
    Ok(path)
}

#[cfg(any(target_os = "macos", all(unix, test)))]
fn ensure_link(link: &Path, target: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(link) {
        Ok(meta)
            if meta.file_type().is_symlink()
                && std::fs::read_link(link).map_err(|e| e.to_string())? == target =>
        {
            Ok(())
        }
        Ok(_) => {
            Err("App keychain routing already exists and differs. It was not replaced.".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !target.is_file() {
                return Err("OS default keychain is unavailable".into());
            }
            std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Strip inherited Mesh overrides so an agent shell cannot redirect the child's
/// trust, identity, plugins or runtime into a different application profile.
pub fn configure(command: &mut std::process::Command, home: &Path) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("MESH_LLM_") {
            command.env_remove(key);
        }
    }
    command
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("MESH_LLM_RUNTIME_ROOT", home.join(".mesh-llm/runtime"));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn os_path_parser_rejects_ambiguous_paths() {
        assert_eq!(
            parse_keychain("  \"/Users/example/Library/Keychains/custom.keychain-db\"\n").unwrap(),
            PathBuf::from("/Users/example/Library/Keychains/custom.keychain-db")
        );
        for value in ["", "relative", "\"relative\"", "\"/a/../b\""] {
            assert!(parse_keychain(value).is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn routing_is_idempotent_and_never_replaces_existing_state() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("os-keychain");
        std::fs::write(&target, "not a real credential").unwrap();
        let link = root.path().join("app-link");
        ensure_link(&link, &target).unwrap();
        ensure_link(&link, &target).unwrap();
        assert!(ensure_link(&link, &root.path().join("other")).is_err());
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, "established").unwrap();
        assert!(ensure_link(&link, &target).is_err());
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "established");
    }
    #[test]
    fn only_child_home_is_changed() {
        let before = std::env::var_os("HOME");
        let mut command = std::process::Command::new("mesh-llm");
        configure(&mut command, Path::new("/app/home"));
        assert_eq!(std::env::var_os("HOME"), before);
        assert!(command
            .get_envs()
            .any(|(k, v)| k == "HOME" && v == Some(std::ffi::OsStr::new("/app/home"))));
    }
}

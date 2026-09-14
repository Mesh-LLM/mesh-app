//! Subprocess-only HOME isolation for the official runtime (no --profile-dir).
//! The GUI retains OS HOME. On macOS only the OS-selected default keychain is
//! linked, not the personal Mesh directory or the entire Library tree. Keychain
//! ACLs still apply; no credential is read, copied, exported or rewritten here.
use std::path::Path;
#[cfg(any(unix, test))]
use std::path::PathBuf;

pub fn prepare(home: &Path) -> Result<(), String> {
    std::fs::create_dir_all(home.join(".mesh-llm")).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    link_shared_caches(home)?;
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

/// Model storage belongs to the user, not to this app profile: the child must
/// read and write the same directories the plain CLI does. Weight storage is
/// carried by `HF_HUB_CACHE`/`HF_XET_CACHE` in `configure`, which sit at the top
/// of the engine's precedence chain. The engine's own cache root and the model
/// catalog have no environment override and are derived from HOME, so the only
/// way to share them under a synthetic HOME is to link them to the real ones.
/// Resolution happens in the untouched parent environment; nothing is guessed.
#[cfg(unix)]
fn link_shared_caches(home: &Path) -> Result<(), String> {
    let cache_root = if cfg!(target_os = "macos") {
        home.join("Library/Caches")
    } else {
        home.join(".cache")
    };
    let mut links = vec![(cache_root.join("mesh-llm"), model_hf::mesh_llm_cache_dir())];
    // Catalog cache: `$HOME/.cache/meshllm/catalog`, HOME-derived on every platform.
    if let Some(real_home) = std::env::var_os("HOME").map(PathBuf::from) {
        if real_home != home {
            links.push((
                home.join(".cache/meshllm"),
                real_home.join(".cache/meshllm"),
            ));
        }
    }
    for (link, target) in links {
        ensure_shared_dir(&link, &target)?;
    }
    Ok(())
}

/// Link a child cache path to the user's real one. An existing private copy is
/// moved aside rather than deleted: duplicated weights are the user's data, and
/// this function must never be the thing that loses them.
#[cfg(any(unix, test))]
fn ensure_shared_dir(link: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    match std::fs::symlink_metadata(link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if std::fs::read_link(link).map_err(|e| e.to_string())? == target {
                Ok(())
            } else {
                Err(format!(
                    "{} already points somewhere else. It was not replaced.",
                    link.display()
                ))
            }
        }
        Ok(_) => {
            let aside = superseded_path(link)?;
            std::fs::rename(link, &aside).map_err(|e| e.to_string())?;
            std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// A never-clobbering sibling name for a superseded private cache.
#[cfg(any(unix, test))]
fn superseded_path(link: &Path) -> Result<PathBuf, String> {
    let name = link
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Cache path has no usable name")?;
    for attempt in 0..1000 {
        let candidate = link.with_file_name(format!("{name}.superseded.{attempt}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Too many superseded cache directories; remove them and retry.".into())
}

/// Strip inherited Mesh overrides so an agent shell cannot redirect the child's
/// trust, identity, plugins or runtime into a different application profile.
pub fn configure(command: &mut std::process::Command, home: &Path) {
    // Use the engine resolver in the untouched parent environment, including OS
    // defaults and explicit hub/Xet overrides. Never guess a platform cache root.
    command
        .env("HF_HUB_CACHE", model_hf::huggingface_hub_cache_dir())
        .env("HF_XET_CACHE", model_hf::huggingface_xet_cache_dir());
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
    fn model_cache_uses_original_environment() {
        let hub = model_hf::huggingface_hub_cache_dir();
        let xet = model_hf::huggingface_xet_cache_dir();
        let mut command = std::process::Command::new("mesh-llm");
        configure(&mut command, Path::new("/app/home"));
        for (key, expected) in [("HF_HUB_CACHE", hub), ("HF_XET_CACHE", xet)] {
            assert!(command
                .get_envs()
                .any(|(k, v)| k == key && v == Some(expected.as_os_str())));
            println!("{key}={}", expected.display());
        }
    }
    #[cfg(unix)]
    #[test]
    fn shared_cache_link_supersedes_a_private_copy_without_deleting_it() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared/mesh-llm");
        let link = root.path().join("home/Library/Caches/mesh-llm");
        // Fresh profile: the link is created and points at the user's cache.
        ensure_shared_dir(&link, &shared).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), shared);
        // Idempotent.
        ensure_shared_dir(&link, &shared).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), shared);
        // A pre-existing private cache is moved aside, never removed.
        std::fs::remove_file(&link).unwrap();
        std::fs::create_dir_all(&link).unwrap();
        std::fs::write(link.join("duplicate"), "weights").unwrap();
        ensure_shared_dir(&link, &shared).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), shared);
        assert_eq!(
            std::fs::read_to_string(link.with_file_name("mesh-llm.superseded.0/duplicate"))
                .unwrap(),
            "weights"
        );
        // A link to somewhere else is reported, not silently repointed.
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(root.path().join("elsewhere"), &link).unwrap();
        assert!(ensure_shared_dir(&link, &shared).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn child_resolves_the_same_mesh_cache_as_the_plain_cli() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("profile-home");
        std::fs::create_dir_all(&home).unwrap();
        prepare(&home).unwrap();
        // The path the child's `dirs::cache_dir()/mesh-llm` will resolve to.
        let child_view = if cfg!(target_os = "macos") {
            home.join("Library/Caches/mesh-llm")
        } else {
            home.join(".cache/mesh-llm")
        };
        let expected = model_hf::mesh_llm_cache_dir();
        assert_eq!(std::fs::read_link(&child_view).unwrap(), expected);
        println!("child mesh cache -> {}", expected.display());
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

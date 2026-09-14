//! Start this tray over: forget its node identity and its remembered people.
//!
//! Scope is deliberately narrow. Downloaded models live in the user's shared
//! caches (see `runtime_home::link_shared_caches`) and are never touched here,
//! nor is the user's own `~/.mesh-llm` CLI identity, nor any other Mesh node.
//! Reset removes tray-profile state only, and refuses to follow a symlink so it
//! can never reach outside the profile even if the profile is tampered with.
use std::path::Path;

/// Profile-relative paths that hold this tray's identity, trust and pairings.
/// Cache links live under `home/Library/Caches` and `home/.cache`, outside every
/// entry below, so no shared model storage is reachable from this list.
const IDENTITY_STATE: &[&str] = &[
    "home/.mesh-llm",
    "public-home/.mesh-llm",
    "public",
    "owner-id",
];

/// Remove this tray's identity and pairings. The caller must stop the child
/// first; this does not signal or wait on a running runtime.
pub fn perform(root: &Path) -> Result<(), String> {
    for relative in IDENTITY_STATE {
        remove_profile_entry(&root.join(relative))?;
    }
    let mut settings = crate::settings::Settings::load(root)?;
    settings.admitted_owners.clear();
    settings.owner_names.clear();
    settings.save(root)?;
    Ok(())
}

/// Delete a profile entry, or report why it was left alone. A symlink is
/// unlinked, never traversed, so deletion cannot escape the profile.
fn remove_profile_entry(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            std::fs::remove_file(path).map_err(|e| e.to_string())
        }
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path).map_err(|e| e.to_string()),
        Ok(_) => std::fs::remove_file(path).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_identity_and_pairings_but_keeps_shared_caches() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        std::fs::create_dir_all(root.join("home/.mesh-llm")).unwrap();
        std::fs::write(root.join("home/.mesh-llm/key"), "node key").unwrap();
        std::fs::write(root.join("home/.mesh-llm/owner-keystore.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("public")).unwrap();
        std::fs::write(root.join("owner-id"), "abc").unwrap();
        let shared = root.join("real-cache");
        std::fs::create_dir_all(shared.join("mesh-llm")).unwrap();
        std::fs::write(shared.join("mesh-llm/model-meta"), "shared").unwrap();
        std::fs::create_dir_all(root.join("home/Library/Caches")).unwrap();
        std::os::unix::fs::symlink(
            shared.join("mesh-llm"),
            root.join("home/Library/Caches/mesh-llm"),
        )
        .unwrap();
        let mut settings = crate::settings::Settings::default();
        settings.admitted_owners.push("ab".repeat(32));
        settings
            .owner_names
            .insert("ab".repeat(32), "Someone".into());
        settings.save(root).unwrap();

        perform(root).unwrap();

        assert!(!root.join("home/.mesh-llm").exists());
        assert!(!root.join("public").exists());
        assert!(!root.join("owner-id").exists());
        let settings = crate::settings::Settings::load(root).unwrap();
        assert!(settings.admitted_owners.is_empty());
        assert!(settings.owner_names.is_empty());
        // The shared cache and its contents survive, link target included.
        assert_eq!(
            std::fs::read_to_string(shared.join("mesh-llm/model-meta")).unwrap(),
            "shared"
        );
        assert!(root.join("home/Library/Caches/mesh-llm").exists());
    }

    #[test]
    fn is_idempotent_on_a_profile_that_was_never_established() {
        let root = tempfile::tempdir().unwrap();
        perform(root.path()).unwrap();
        perform(root.path()).unwrap();
        assert!(!root.path().join("owner-id").exists());
    }

    #[test]
    fn a_symlinked_identity_directory_is_unlinked_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precious"), "keep").unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("home/.mesh-llm")).unwrap();

        perform(root).unwrap();

        assert!(!root.join("home/.mesh-llm").exists());
        assert_eq!(
            std::fs::read_to_string(outside.join("precious")).unwrap(),
            "keep"
        );
    }
}

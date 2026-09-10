//! Stable app-profile ownership. Never consult the user's CLI identity.
use mesh_llm_identity::{
    keystore_metadata, load_keystore, load_owner_keypair_from_keychain,
    save_keystore_with_keychain, OwnerKeypair,
};
use std::{fs::File, io::Write, path::Path};

/// Hold for the entire app lifetime, including child shutdown and native dialogs.
/// OS locks release on crash; a leftover lock file is harmless.
pub fn lock_profile(root: &Path) -> Result<File, String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("profile.lock"))
        .map_err(|e| e.to_string())?;
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|_| "This Mesh profile is already open. Use its existing tray.".to_string())?;
    Ok(file)
}

pub fn ensure(root: &Path) -> Result<OwnerKeypair, String> {
    ensure_with(root, &NativeStore)
}
trait Store {
    fn load(&self, path: &Path) -> Result<OwnerKeypair, String>;
    fn create(&self, path: &Path, owner: &OwnerKeypair) -> Result<(), String>;
}
struct NativeStore;
impl Store for NativeStore {
    fn load(&self, path: &Path) -> Result<OwnerKeypair, String> {
        let info = keystore_metadata(path).map_err(|_| {
            "Mesh identity cannot be read. Restore this profile; it was not replaced.".to_string()
        })?;
        if info.encrypted {
            load_owner_keypair_from_keychain(path).map_err(|_| "Unlock or allow access to your OS credential store, then retry. If this profile was moved, restore its unlock credential. Your identity was not replaced.".into())
        } else {
            load_keystore(path, None)
                .map_err(|_| "Mesh identity is damaged; restore this profile.".into())
        }
    }
    fn create(&self, path: &Path, owner: &OwnerKeypair) -> Result<(), String> {
        // No plaintext fallback and no availability probe that changes credentials.
        save_keystore_with_keychain(path, owner, false).map(|_| ())
            .map_err(|_| "Could not securely set up Mesh. Unlock or enable your OS credential store and retry. No unprotected identity was created.".into())
    }
}
fn ensure_with(root: &Path, store: &impl Store) -> Result<OwnerKeypair, String> {
    let directory = root.join("home/.mesh-llm");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let path = directory.join("owner-keystore.json");
    let marker = root.join("owner-id");
    let expected = match std::fs::read_to_string(&marker) {
        Ok(id) => Some(id),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let owner = match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => store.load(&path)?,
        Ok(_) => return Err("Mesh identity must be a regular file, not a link.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Existing attestations also distinguish legacy established profiles.
            if expected.is_some() || directory.join("node-ownership.json").exists() {
                return Err("This profile's identity is missing. Restore it; Mesh will not silently create a different identity.".into());
            }
            let owner = OwnerKeypair::generate();
            store.create(&path, &owner)?;
            // Verify storage round trip before recording a stable identity.
            let loaded = store.load(&path)?;
            if loaded.owner_id() != owner.owner_id() {
                return Err("Stored Mesh identity did not match setup.".into());
            }
            loaded
        }
        Err(e) => return Err(e.to_string()),
    };
    let id = owner.owner_id();
    if let Some(expected) = expected {
        if expected != id {
            return Err(
                "Mesh identity changed unexpectedly. Restore this profile before continuing."
                    .into(),
            );
        }
    } else {
        let mut file = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
        file.write_all(id.as_bytes()).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist_noclobber(marker).map_err(|e| e.to_string())?;
    }
    Ok(owner)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestStore;
    impl Store for TestStore {
        fn load(&self, p: &Path) -> Result<OwnerKeypair, String> {
            load_keystore(p, None).map_err(|e| e.to_string())
        }
        fn create(&self, p: &Path, o: &OwnerKeypair) -> Result<(), String> {
            mesh_llm_identity::save_keystore(p, o, None, false).map_err(|e| e.to_string())
        }
    }
    #[test]
    fn creates_once_and_missing_established_identity_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let owner = ensure_with(root.path(), &TestStore).unwrap().owner_id();
        assert_eq!(
            ensure_with(root.path(), &TestStore).unwrap().owner_id(),
            owner
        );
        std::fs::remove_file(root.path().join("home/.mesh-llm/owner-keystore.json")).unwrap();
        assert!(ensure_with(root.path(), &TestStore).is_err());
    }
    #[test]
    fn corruption_and_replacement_never_generate_another_owner() {
        let root = tempfile::tempdir().unwrap();
        ensure_with(root.path(), &TestStore).unwrap();
        let p = root.path().join("home/.mesh-llm/owner-keystore.json");
        std::fs::write(&p, "broken").unwrap();
        assert!(ensure_with(root.path(), &TestStore).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "broken");
        mesh_llm_identity::save_keystore(&p, &OwnerKeypair::generate(), None, true).unwrap();
        assert!(ensure_with(root.path(), &TestStore).is_err());
    }
    #[test]
    fn profile_lock_is_exclusive_and_releases() {
        let root = tempfile::tempdir().unwrap();
        let guard = lock_profile(root.path()).unwrap();
        assert!(lock_profile(root.path()).is_err());
        drop(guard);
        assert!(lock_profile(root.path()).is_ok());
    }
    #[test]
    fn storage_failure_does_not_establish_a_profile() {
        struct Denied;
        impl Store for Denied {
            fn load(&self, _: &Path) -> Result<OwnerKeypair, String> {
                Err("denied".into())
            }
            fn create(&self, _: &Path, _: &OwnerKeypair) -> Result<(), String> {
                Err("denied".into())
            }
        }
        let root = tempfile::tempdir().unwrap();
        assert!(ensure_with(root.path(), &Denied).is_err());
        assert!(!root.path().join("owner-id").exists());
        assert!(ensure_with(root.path(), &TestStore).is_ok());
    }
}

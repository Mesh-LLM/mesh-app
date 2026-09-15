//! Stable app-profile ownership. Never consult the user's CLI identity.
use mesh_llm_identity::{
    keystore_metadata, load_keystore, load_owner_keypair_from_keychain,
    save_keystore_with_keychain, OwnerKeypair,
};
use std::{fs::File, path::Path};

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

/// Confirm this profile has a usable identity **without reading its secret**.
///
/// Startup only needs to know the profile is established and which owner it is;
/// the runtime child is the component that actually unlocks the key. Reading the
/// secret here would make the user approve a second Keychain prompt for the same
/// identity every launch. The owner id returned by `keystore_metadata` is
/// verified against the keystore's signing public key, so this is a real check,
/// not a file-exists test.
pub fn establish(root: &Path) -> Result<String, String> {
    establish_with(root, &NativeStore)
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
/// This machine's one owner keystore, the same file the plain CLI and Buzz use
/// (`desktop/src-tauri/src/mesh_llm/identity.rs:1-8`). The tray never keeps a
/// second identity, so your friends see one node from this machine whichever
/// way Mesh was started.
fn keystore_path(root: &Path) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    Ok(root.join("owner-keystore.json"))
}

fn establish_with(root: &Path, store: &impl Store) -> Result<String, String> {
    let path = keystore_path(root)?;
    match std::fs::symlink_metadata(&path) {
        // Established: read public metadata only, so no credential is unlocked.
        Ok(meta) if meta.is_file() => {
            let info = keystore_metadata(&path).map_err(|_| {
                "Mesh identity cannot be read. Restore this profile; it was not replaced."
                    .to_string()
            })?;
            Ok(info.owner_id)
        }
        Ok(_) => Err("Mesh identity must be a regular file, not a link.".into()),
        // Not established yet: creating one is the only path that touches the
        // credential store, and it happens once per machine.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(ensure_with(root, store)?.owner_id())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn ensure_with(root: &Path, store: &impl Store) -> Result<OwnerKeypair, String> {
    let path = keystore_path(root)?;
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => store.load(&path),
        Ok(_) => Err("Mesh identity must be a regular file, not a link.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // A node that has served under an identity must never be handed a
            // different one behind the user's back. Only a profile with no
            // history at all gets a fresh key.
            if root.join("node-ownership.json").exists() {
                return Err("This machine's Mesh identity is missing. Restore it; Mesh will not silently create a different identity.".into());
            }
            let owner = OwnerKeypair::generate();
            store.create(&path, &owner)?;
            // Verify storage round trip before relying on the identity.
            let loaded = store.load(&path)?;
            if loaded.owner_id() != owner.owner_id() {
                return Err("Stored Mesh identity did not match setup.".into());
            }
            Ok(loaded)
        }
        Err(e) => Err(e.to_string()),
    }
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
    fn startup_verification_never_reads_the_secret() {
        /// Establishes normally, then refuses every secret read afterwards.
        struct CreateOnly;
        impl Store for CreateOnly {
            fn load(&self, _: &Path) -> Result<OwnerKeypair, String> {
                Err("startup must not unlock the credential store".into())
            }
            fn create(&self, p: &Path, o: &OwnerKeypair) -> Result<(), String> {
                mesh_llm_identity::save_keystore(p, o, None, false).map_err(|e| e.to_string())
            }
        }
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let owner = ensure_with(root, &TestStore).unwrap().owner_id();
        // Established profile: no load, so no Keychain prompt on launch.
        assert_eq!(establish_with(root, &CreateOnly).unwrap(), owner);
        assert_eq!(establish_with(root, &CreateOnly).unwrap(), owner);
    }

    #[test]
    fn the_identity_is_the_machine_profile_the_cli_already_uses() {
        let root = tempfile::tempdir().unwrap();
        let id = establish_with(root.path(), &TestStore).unwrap();
        // One file, in the profile root -- no app-owned second keystore.
        let path = root.path().join("owner-keystore.json");
        assert!(path.is_file());
        assert_eq!(ensure_with(root.path(), &TestStore).unwrap().owner_id(), id);
        // Whatever identity the profile holds is the one the tray uses, so a
        // profile the user reset with the CLI is adopted rather than refused.
        let replacement = OwnerKeypair::generate();
        mesh_llm_identity::save_keystore(&path, &replacement, None, true).unwrap();
        assert_eq!(
            establish_with(root.path(), &TestStore).unwrap(),
            replacement.owner_id()
        );
    }

    #[test]
    fn creates_once_and_a_served_profile_missing_its_key_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let owner = ensure_with(root.path(), &TestStore).unwrap().owner_id();
        assert_eq!(
            ensure_with(root.path(), &TestStore).unwrap().owner_id(),
            owner
        );
        let path = root.path().join("owner-keystore.json");
        std::fs::remove_file(&path).unwrap();
        // No history: a fresh key is correct, which is what "start over" needs.
        assert!(ensure_with(root.path(), &TestStore).is_ok());
        std::fs::remove_file(&path).unwrap();
        // History: refuse, never hand this node a different identity silently.
        std::fs::write(root.path().join("node-ownership.json"), "{}").unwrap();
        assert!(ensure_with(root.path(), &TestStore).is_err());
    }
    #[test]
    fn corruption_is_reported_and_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        ensure_with(root.path(), &TestStore).unwrap();
        let p = root.path().join("owner-keystore.json");
        std::fs::write(&p, "broken").unwrap();
        assert!(ensure_with(root.path(), &TestStore).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "broken");
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
        assert!(!root.path().join("owner-keystore.json").exists());
        assert!(ensure_with(root.path(), &TestStore).is_ok());
    }
}

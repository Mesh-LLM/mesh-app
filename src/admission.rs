//! Projection of the tray's approved owner list into its child-only Mesh home.
//! Mesh still owns certificate validation and Allowlist enforcement.
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

pub fn validate_owners(owners: &[String]) -> Result<(), String> {
    if owners.len() > 1024 {
        return Err("Too many admitted identities".into());
    }
    for (index, owner) in owners.iter().enumerate() {
        // Mesh owner IDs are SHA-256 public-key hashes, serialized lowercase.
        if owner.len() != 64
            || !owner
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || owners[..index].contains(owner)
        {
            return Err("Invalid or duplicate admitted identity".into());
        }
    }
    Ok(())
}

/// Call only while this app's child is stopped. Replace positive grants, not
/// revocations; a removed owner must not return via Mesh's startup merge.
/// Keep the disk policy unchanged: Private explicitly overrides it with
/// --trust-policy allowlist, whereas Public retains its previous policy.
pub fn prepare_store(home: &Path, owners: &[String]) -> Result<(), String> {
    validate_owners(owners)?;
    let directory = home.join(".mesh-llm");
    let path = directory.join("trusted-owners.json");
    let mut store: Value = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Cannot read private trust store: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({
            "version": 1, "policy": "off", "trusted_owners": [],
            "revoked_owners": [], "revoked_node_certs": [], "revoked_node_ids": []
        }),
        Err(e) => return Err(format!("Cannot read private trust store: {e}")),
    };
    let object = store.as_object_mut().ok_or("Invalid private trust store")?;
    if object.get("version") != Some(&json!(1)) {
        return Err("Unsupported private trust store version; left unchanged".into());
    }
    // Validate the known Mesh fields before overwriting anything. Unknown fields
    // and all revocations survive the projection for forward compatibility.
    if !matches!(
        object.get("policy").and_then(Value::as_str),
        Some("off" | "prefer-owned" | "require-owned" | "allowlist")
    ) {
        return Err("Invalid private trust policy; left unchanged".into());
    }
    for key in [
        "trusted_owners",
        "revoked_owners",
        "revoked_node_certs",
        "revoked_node_ids",
    ] {
        if object.get(key).is_some_and(|value| !value.is_array()) {
            return Err(format!("Invalid {key}; private trust store left unchanged"));
        }
    }
    object.insert(
        "trusted_owners".into(),
        Value::Array(
            owners
                .iter()
                .map(|owner| json!({"owner_id": owner}))
                .collect(),
        ),
    );
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(&store).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(home: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(home.join(".mesh-llm/trusted-owners.json")).unwrap())
            .unwrap()
    }

    #[test]
    fn removal_survives_repeated_startup_projection() {
        let home = tempfile::tempdir().unwrap();
        let approved = "ab".repeat(32);
        prepare_store(home.path(), std::slice::from_ref(&approved)).unwrap();
        assert_eq!(read(home.path())["trusted_owners"][0]["owner_id"], approved);
        prepare_store(home.path(), &[]).unwrap();
        prepare_store(home.path(), &[]).unwrap();
        assert_eq!(read(home.path())["trusted_owners"], json!([]));
        assert_eq!(read(home.path())["policy"], "off");
    }

    #[test]
    fn replaces_stale_grants_but_preserves_revocations_and_policy() {
        let home = tempfile::tempdir().unwrap();
        prepare_store(home.path(), &["ab".repeat(32)]).unwrap();
        let path = home.path().join(".mesh-llm/trusted-owners.json");
        let mut store = read(home.path());
        store["revoked_owners"] = json!([{"owner_id": "cd".repeat(32)}]);
        store["policy"] = json!("require-owned");
        store["future_field"] = json!(true);
        std::fs::write(&path, store.to_string()).unwrap();
        prepare_store(home.path(), &[]).unwrap();
        store["trusted_owners"] = json!([]);
        assert_eq!(read(home.path()), store);
    }

    #[test]
    fn corrupt_store_is_not_replaced() {
        let home = tempfile::tempdir().unwrap();
        prepare_store(home.path(), &[]).unwrap();
        let path = home.path().join(".mesh-llm/trusted-owners.json");
        for bytes in [
            "bad json",
            r#"{"version":2}"#,
            r#"{"version":1,"policy":"off","revoked_owners":false}"#,
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(prepare_store(home.path(), &[]).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn invalid_roster_is_rejected() {
        for owners in [
            vec!["".into()],
            vec!["AB".repeat(32)],
            vec!["ab".repeat(32); 2],
        ] {
            assert!(validate_owners(&owners).is_err());
        }
    }
}

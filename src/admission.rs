//! Validation of the owner identities this tray has admitted.
//!
//! The list is declared to the engine on the command line (`--trust-owner`,
//! `mesh-llm-cli/src/parser/normalization.rs:69`), the same way Buzz declares
//! its roster through the SDK. The engine merges those arguments with the
//! machine's trust store in memory and writes nothing back
//! (`mesh-llm-host-runtime/src/runtime/startup_models.rs:141`), so the tray
//! never edits the user's trusted owners -- and cannot revoke one either: the
//! effective allowlist is the union. Mesh still owns certificate validation and
//! Allowlist enforcement.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_roster_is_rejected() {
        for owners in [
            vec!["".into()],
            vec!["AB".repeat(32)],
            vec!["ab".repeat(32); 2],
        ] {
            assert!(validate_owners(&owners).is_err());
        }
        validate_owners(&["ab".repeat(32), "cd".repeat(32)]).unwrap();
    }
}

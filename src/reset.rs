//! Start this tray over: forget the people and the invitations it remembers.
//!
//! All of that state is launcher state, in `~/.mesh-app/launcher.json`: who was
//! admitted, what they are called, which invitations are outstanding, which
//! Mesh seeds were accepted, and the mode. The tray declares its roster to the
//! engine on the command line, so clearing this file is the whole reset -- the
//! next start declares nobody. Grants the user or Buzz made in their own
//! `~/.mesh-llm/trusted-owners.json` are theirs and survive, because the engine
//! merges that store with the tray's arguments; removing one is `mesh-llm auth`.
//!
//! Deliberately untouched: your machine's Mesh identity in `~/.mesh-llm`, which
//! the plain CLI and Buzz share (resetting that is `rm -rf ~/.mesh-llm`, and it
//! resets them too), your engine `config.toml`, and downloaded models.
use std::path::Path;

/// Forget everyone this tray paired with. The caller must stop the child first;
/// this does not signal or wait on a running runtime.
pub fn perform(root: &Path) -> Result<(), String> {
    // Keep the ports, because they describe this installation rather than any
    // relationship, and a reset that moved the console would look like a fault.
    let current = crate::settings::Settings::load(root)?;
    crate::settings::Settings {
        console_port: current.console_port,
        api_port: current.api_port,
        ..Default::default()
    }
    .save(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Connection, Settings};

    #[test]
    fn forgets_people_invitations_and_mode_but_keeps_ports() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let owner = "ab".repeat(32);
        let mut settings = Settings {
            connection: Connection::Private {
                invite: Some("invite".into()),
            },
            admitted_owners: vec![owner.clone()],
            seeds: vec!["seed".into()],
            issued_membership_invitations: vec!["issued".into()],
            console_port: 4242,
            api_port: 4243,
            ..Default::default()
        };
        settings.owner_names.insert(owner, "Someone".into());
        settings.save(root).unwrap();

        perform(root).unwrap();

        let after = Settings::load(root).unwrap();
        assert!(after.admitted_owners.is_empty());
        assert!(after.owner_names.is_empty());
        assert!(after.seeds.is_empty());
        assert!(after.issued_membership_invitations.is_empty());
        assert_eq!(after.connection, Connection::Automatic);
        assert_eq!((after.console_port, after.api_port), (4242, 4243));
        // Nothing the engine owns is reachable from a reset.
        assert!(!after.args().contains(&"--trust-owner".into()));
    }

    #[test]
    fn is_idempotent_on_a_tray_that_never_paired() {
        let root = tempfile::tempdir().unwrap();
        perform(root.path()).unwrap();
        perform(root.path()).unwrap();
        assert!(Settings::load(root.path())
            .unwrap()
            .admitted_owners
            .is_empty());
    }
}

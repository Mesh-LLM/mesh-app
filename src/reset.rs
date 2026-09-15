//! Changing mode forgets this Mesh. There is one concept, not two.
//!
//! Everything the tray remembers is one relationship: who is trusted, what they
//! are called, which invitations are outstanding, which Mesh seeds were
//! accepted. That set belongs to the private Mesh you were running, so leaving
//! it — going Public, or starting a new Private one — forgets it. Public trusts
//! nobody in particular, so there is nothing left over to reset separately.
//!
//! The roster reaches the engine as `--trust-owner` arguments, so forgetting is
//! complete: the next start declares nobody. Grants the user or Buzz made in
//! their own `~/.mesh-llm/trusted-owners.json` are theirs and survive, because
//! the engine merges that store with the tray's arguments; removing one is
//! `mesh-llm auth`. The machine's Mesh identity, the engine config and
//! downloaded models are never touched.
use crate::settings::{Connection, Settings};

/// The settings to start in `connection`, having forgotten the Mesh you were in.
///
/// Ports are kept, because they describe this installation rather than any
/// relationship, and a switch that moved the console would look like a fault.
pub fn switching_to(current: &Settings, connection: Connection) -> Settings {
    Settings {
        connection,
        console_port: current.console_port,
        api_port: current.api_port,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paired() -> Settings {
        let owner = "ab".repeat(32);
        let mut settings = Settings {
            connection: Connection::Private {
                invite: Some("invite".into()),
            },
            admitted_owners: vec![owner.clone()],
            seeds: vec!["seed".into()],
            issued_membership_invitations: vec!["issued".into()],
            membership_receipt: Some(b"card".to_vec()),
            console_port: 4242,
            api_port: 4243,
            ..Default::default()
        };
        settings.owner_names.insert(owner, "Someone".into());
        settings
    }

    #[test]
    fn going_public_forgets_who_was_trusted_and_keeps_the_ports() {
        let after = switching_to(&paired(), Connection::Automatic);
        after.validate().unwrap();
        assert_eq!(after.connection, Connection::Automatic);
        assert!(after.admitted_owners.is_empty());
        assert!(after.owner_names.is_empty());
        assert!(after.seeds.is_empty());
        assert!(after.issued_membership_invitations.is_empty());
        assert!(after.membership_receipt.is_none());
        assert_eq!((after.console_port, after.api_port), (4242, 4243));
        // Nothing is declared to the engine, so nobody is trusted on restart.
        assert!(!after.args().contains(&"--trust-owner".into()));
    }

    #[test]
    fn a_new_private_mesh_starts_with_nobody_in_it() {
        let after = switching_to(&paired(), Connection::Private { invite: None });
        after.validate().unwrap();
        assert_eq!(after.connection, Connection::Private { invite: None });
        assert!(after.admitted_owners.is_empty());
        assert!(after.seeds.is_empty());
        assert!(!after.args().contains(&"--trust-owner".into()));
        assert!(after.args().contains(&"--owner-required".into()));
    }
}

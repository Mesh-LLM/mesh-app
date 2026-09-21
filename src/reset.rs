//! Changing mode forgets this Mesh. There is one concept, not two.
//!
//! Everything the tray remembers about a private Mesh is the invites that put
//! this node in it. That set belongs to the Mesh you were running, so leaving
//! it — going Public, or starting a new Private one — forgets it. Public trusts
//! nobody in particular, so there is nothing left over to reset separately.
//!
//! Forgetting is complete on the tray's side: the next start declares no
//! invite, so it creates its own Mesh rather than rejoining theirs. It is not a
//! revocation, and nothing claims it is: people who hold a valid invite to a
//! Mesh you created can still use it, because the engine has no mesh-wide
//! eviction. The machine's Mesh identity, the engine config and downloaded
//! models are never touched.
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined() -> Settings {
        Settings {
            connection: Connection::Private {
                invite: Some("invite".into()),
            },
            console_port: 4242,
            api_port: 4243,
        }
    }

    #[test]
    fn going_public_forgets_the_mesh_and_keeps_the_ports() {
        let after = switching_to(&joined(), Connection::Automatic);
        after.validate().unwrap();
        assert_eq!(after.connection, Connection::Automatic);
        assert!(after.joins().is_empty());
        assert_eq!((after.console_port, after.api_port), (4242, 4243));
        assert!(!after.args().contains(&"--join".into()));
    }

    #[test]
    fn a_new_private_mesh_is_its_own_mesh_not_the_one_just_left() {
        let after = switching_to(&joined(), Connection::Private { invite: None });
        after.validate().unwrap();
        assert_eq!(after.connection, Connection::Private { invite: None });
        assert!(!after.args().contains(&"--join".into()));
        // No invite left means this node creates: it declares the requirement.
        assert!(after.args().contains(&"--min-node-version".into()));
        assert!(after.args().contains(&"--owner-required".into()));
    }
}

//! Connection changes only replace launcher preferences.
//! Explicit Reset additionally retires engine membership through `private_reset`
//! after verified shutdown; this function alone does not revoke membership.
use crate::settings::{Connection, Settings};

/// The settings to start in `connection`, having forgotten the Mesh you were in.
///
/// Ports are kept, because they describe this installation rather than any
/// relationship, and a switch that moved the console would look like a fault.
pub fn switching_to(current: &Settings, connection: Connection) -> Settings {
    Settings {
        connection,
        share_compute: current.share_compute,
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
            share_compute: false,
            console_port: 4242,
            api_port: 4243,
        }
    }

    #[test]
    fn going_public_forgets_the_mesh_and_keeps_the_ports() {
        let after = switching_to(&joined(), Connection::Automatic);
        after.validate().unwrap();
        assert_eq!(after.connection, Connection::Automatic);
        assert!(!after.share_compute);
        assert!(after.joins().is_empty());
        assert_eq!((after.console_port, after.api_port), (4242, 4243));
    }

    #[test]
    fn switching_private_clears_the_launcher_invite() {
        let after = switching_to(&joined(), Connection::Private { invite: None });
        after.validate().unwrap();
        assert_eq!(after.connection, Connection::Private { invite: None });
        assert!(after.joins().is_empty());
        // SDK policy mapping is covered by lifecycle configuration tests.
    }
}

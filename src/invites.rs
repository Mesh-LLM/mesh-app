//! One code, both directions.
//!
//! A Private Mesh here is requirement-aware, so its invite is a signed bearer
//! token: hand it to anyone, they paste it, and they are in — trusted by
//! everybody already in, with no reply, no approval and no list. Forwarding
//! works for the same reason, and a node that joined can only hand back the
//! token it was given, never mint a new one
//! (`mesh-llm-host-runtime/src/mesh/node_identity.rs:143-185`).
//!
//! So the tray needs exactly two actions, and neither of them is a ceremony:
//! share the code, or paste one.
use crate::{native, status, App};
use mesh_tray::settings::{self, Connection};

impl App {
    /// Share this node's invite on macOS; other platforms copy it.
    /// The same private-mesh, owner and child-PID guards apply everywhere.
    pub(crate) fn invite(&mut self) {
        let result = (|| {
            let token = self.mint_invite()?;
            native::share_text(&token)?;
            Ok::<(), String>(())
        })();
        if let Err(e) = result {
            native::notice("Could not create an invite", &e);
        }
    }

    /// Mint (or re-share) this node's signed bearer invite.
    fn mint_invite(&mut self) -> Result<String, String> {
        if self.stopping.is_some() || self.pending_settings.is_some() {
            return Err("Wait for Mesh to finish restarting".into());
        }
        if !matches!(self.settings.connection, Connection::Private { .. }) {
            return Err(
                "Invites belong to a private Mesh. Choose Private first, then invite people."
                    .into(),
            );
        }
        // The status owner is the running child's verified identity. Using
        // it instead of unlocking the keystore keeps the promise of one
        // credential prompt per launch; the PID check below is what ties
        // the token to the child this app started.
        let owner = self
            .snapshot
            .private_owner
            .clone()
            .ok_or("Mesh is still getting ready. Try again when it says Ready.")?;
        let pid = self
            .child
            .as_ref()
            .ok_or("Start Private Mesh before inviting")?
            .id();
        status::private_invite(self.settings.console_port, pid, &owner)
    }

    /// Paste an invite and join. Nothing is sent back, so there is no second leg
    /// to remember and nothing for the other person to confirm.
    pub(crate) fn join(&mut self) {
        let result = (|| {
            if self.stopping.is_some() || self.pending_settings.is_some() {
                return Err("Wait for Mesh to finish restarting".into());
            }
            let Some(pasted) = native::prompt_card(
                "Join a private Mesh",
                "Paste the invite you were sent. This replaces your current Mesh selection and restarts Mesh. Your models and settings are kept.",
                "Join",
            ) else {
                return Ok(());
            };
            let token = pasted.trim();
            settings::validate_invite(token)?;
            // Explicit joining replaces the prior mesh selection. Ordinary
            // startup keeps the saved selection for membership restoration.
            let mut next = self.settings.clone();
            next.accept_seed(token)?;
            let profile = settings::mesh_profile()?;
            mesh_tray::identity::establish(&profile)?;
            self.queue_settings(next);
            Ok::<(), String>(())
        })();
        if let Err(e) = result {
            native::notice("Could not join", &e);
        }
    }
}

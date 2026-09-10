//! Native transport controller. Opening verifies, but this draft cannot approve.
use crate::{native, App};
use mesh_tray::{exchange, share_file};
use std::path::Path;

fn now() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis()
        .try_into()
        .map_err(|_| "Invalid clock".into())
}

impl App {
    fn share_ready(&self) -> Result<(), String> {
        if self.stopping.is_some() || self.pending_settings.is_some() {
            return Err("Wait for the connection change to finish, then try again.".into());
        }
        Ok(())
    }

    fn owner(&self) -> Result<mesh_llm_identity::OwnerKeypair, String> {
        // Match the child's isolated HOME. Never use the control machine's identity.
        mesh_llm_identity::load_keystore(&self.root.join("home/.mesh-llm/owner-keystore.json"), None)
            .map_err(|_| "This app needs an unlocked private-profile Mesh owner identity. Native identity setup is not implemented in this draft; your existing identity was not changed.".into())
    }

    pub(crate) fn share_request(&mut self) {
        let result = (|| {
            self.share_ready()?;
            let owner = self.owner()?;
            if !native::confirm("Request to join a private Mesh?", "Share your public Mesh identity with someone you know. This does not share a private key, approve anyone, or change your connection. A response still needs your approval.", "Share request") { return Ok(()); }
            let now = now()?;
            let mut next = self.settings.clone();
            let (bytes, pending) = exchange::create_request(
                &owner,
                "Mesh user",
                None,
                next.exchange.generation(),
                now,
            )?;
            let file = share_file::stage(&bytes)?;
            next.exchange.add_pending(pending, now)?;
            // The pending correlation must exist on disk before a file leaves this app.
            next.save(&self.root)?;
            self.settings = next;
            let tray = &self.ui.as_ref().ok_or("Tray is not ready")?._tray;
            self.native.share(file, tray)
        })();
        if let Err(e) = result {
            native::notice("Could not share request", &e);
        }
    }

    pub(crate) fn review_file(&mut self, path: &Path) {
        let result = (|| {
            self.share_ready()?;
            let bytes = share_file::read(path)?;
            let owner = self.owner()?;
            let now = now()?;
            if let Ok(request) = exchange::verify_request(&bytes, &owner.owner_id(), now) {
                native::notice("Mesh request verified — not approved", &format!("Claimed name: {}\nOwner identity:\n{}\n\nConfirm this identity through your known conversation. Approval and response sharing are not wired in this draft. No access was granted.", request.claimed_name(), request.owner_id()));
            } else {
                let response = self.settings.exchange.verify(&owner, &bytes, now)?;
                native::notice("Mesh response verified — not joined", &format!("Owner identity:\n{}\n\nConfirm this identity through your known conversation. Join approval is not wired in this draft. Your connection and admitted identities are unchanged.", response.owner_id()));
            }
            Ok::<(), String>(())
        })();
        if let Err(e) = result {
            native::notice("Could not open Mesh file", &e);
        }
    }

    pub(crate) fn cancel_requests(&mut self) {
        let result = (|| {
            self.share_ready()?;
            if !native::confirm("Cancel all pending requests?", "Previously shared requests can no longer be used to join from this app. Files already sent cannot be recalled. This does not remove any existing admitted identity.", "Cancel requests") { return Ok(()); }
            let mut next = self.settings.clone();
            next.exchange.invalidate()?;
            next.save(&self.root)?;
            self.settings = next;
            Ok::<(), String>(())
        })();
        if let Err(e) = result {
            native::notice("Could not cancel requests", &e);
        }
    }
}

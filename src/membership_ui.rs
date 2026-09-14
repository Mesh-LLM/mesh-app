//! Native Invitation → RSVP → You're on the list journey, in party terms: the
//! inviter sends a card, the guest RSVPs (which grants nothing), and only the
//! inviter's confirmation puts that exact identity on the list. File delivery
//! stays explicit -- the OS share sheet works on any network, or none.
use crate::{native, App};
use mesh_tray::{identity, invitation, share_file};
fn now() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis()
        .try_into()
        .map_err(|_| "Invalid clock".into())
}
impl App {
    pub(crate) fn invite_member(&mut self) {
        let result = (|| {
            if self.stopping.is_some() || self.pending_settings.is_some() {
                return Err("Wait for Mesh to finish restarting".into());
            }
            if !native::confirm(
                "Invite someone to your Mesh",
                "Three cards, like a party invitation:\n\n1. You send this invitation. It grants no access on its own.\n2. They RSVP. That sends you their identity — they are still not in.\n3. You confirm it is really them. That puts them on the list.\n\nSend the card however you like — Messages, Mail, AirDrop. The invitation expires in 30 minutes.",
                "Create invitation",
            ) { return Ok(()); }
            let owner = identity::ensure(&self.root)?;
            let pid = self
                .child
                .as_ref()
                .ok_or("Start Private Mesh before inviting")?
                .id();
            let seed =
                crate::status::private_invite(self.settings.console_port, pid, &owner.owner_id())?;
            let bytes = invitation::create(&owner, &self.settings, &seed, now()?)?;
            let next = invitation::remember_invitation(&self.settings, &bytes, now()?)?;
            next.save(&self.root)?;
            self.settings = next;
            self.native.share(
                share_file::stage(&bytes)?,
                &self.ui.as_ref().ok_or("Tray unavailable")?._tray,
            )
        })();
        if let Err(e) = result {
            native::notice("Could not invite member", &e);
        }
    }
    pub(crate) fn share_membership(&mut self) {
        let result = (|| {
            if self.stopping.is_some() || self.pending_settings.is_some() {
                return Err("Wait for the connection change to finish before sharing".into());
            }
            let bytes = self.settings.membership_receipt.as_ref().ok_or(
                "Nothing to send yet. Invite someone, or open an invitation and RSVP to it first.",
            )?;
            let value: serde_json::Value =
                serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            if value["mesh_pool_file"] == "approval" {
                let owner = identity::ensure(&self.root)?;
                let pid = self
                    .child
                    .as_ref()
                    .ok_or("Private runtime must be ready before sharing approval")?
                    .id();
                crate::status::private_invite(self.settings.console_port, pid, &owner.owner_id())?;
            }
            if invitation::card_kind(bytes) == Some("confirmation") {
                native::notice(
                    "They're on the list — send them this confirmation",
                    "You have added them. They are not able to join until they open this confirmation card, so send it back the same way the RSVP arrived.",
                );
            } else {
                native::notice(
                    "RSVP ready to send — you have not joined yet",
                    "Send this RSVP back to whoever invited you. They confirm you, then send you a confirmation card. Open that and you are in.",
                );
            }
            self.native.share(
                share_file::stage(bytes)?,
                &self.ui.as_ref().ok_or("Tray unavailable")?._tray,
            )
        })();
        if let Err(e) = result {
            native::notice("Could not share membership file", &e);
        }
    }
    pub(crate) fn review_membership_file(
        &mut self,
        bytes: &[u8],
        owner: &mesh_llm_identity::OwnerKeypair,
        time: u64,
    ) -> Result<bool, String> {
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let Some(kind) = value.get("mesh_pool_file") else {
            return Ok(false);
        };
        match kind.as_str() {
            Some("invitation") => {
                let invite = invitation::inspect(bytes, time)?;
                if !native::confirm(
                    "RSVP to this invitation?",
                    &format!("Invitation from: {}\n{} people already on their list.\n\nRSVP sends them your identity. It does not join you: they must confirm you first, and then send you a confirmation card to open.\n\nA name is not proof of who this is. Your Mesh, your serving and your connections stay exactly as they are.", invite.inviter(), invite.member_count()),
                    "RSVP",
                ) { return Ok(true); }
                let next = invitation::accept(owner, &self.settings, bytes, now()?)?;
                next.save(&self.root)?;
                self.settings = next;
                self.share_membership();
            }
            Some("acceptance") => {
                // Verifies the reply against the invitation; the code itself is not shown.
                let (member, _) = invitation::matching_code(bytes, time)?;
                let Some(allow) = native::decision(
                    "Is this really them?",
                    &format!("Someone has RSVP'd to your invitation.\n\nIdentity: {member}\n\nConfirm only if you are expecting this — a name is not proof, and only you know whether you asked them. Confirming puts this exact identity on your list and hands you a confirmation card to send back. Mesh restarts itself; that takes a moment and needs nothing from you."),
                    "Confirm",
                ) else { return Ok(true); };
                let next =
                    invitation::decide_acceptance(owner, &self.settings, bytes, allow, now()?)?;
                if allow {
                    self.queue_settings(next);
                } else {
                    next.save(&self.root)?;
                    self.settings = next;
                }
            }
            Some("approval") => {
                let next =
                    invitation::apply_receipt(&owner.owner_id(), &self.settings, bytes, now()?)?;
                self.queue_settings(next);
                native::notice(
                    "You're on the list",
                    "Their confirmation checked out and you have been added. Mesh is restarting to pick it up; that takes a moment and needs nothing from you.",
                );
            }
            _ => return Err("Unsupported membership file".into()),
        }
        Ok(true)
    }
}

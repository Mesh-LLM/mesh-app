//! Native Invitation → RSVP → connected journey, in party terms. Two cards:
//! the inviter sends an invitation, and the guest RSVPs. Accepting the
//! invitation joins the guest to the inviter alone; confirming the RSVP admits
//! that exact identity on the inviter's side, which is what connects them. A
//! third card exists and is now optional -- it introduces the inviter's other
//! members. File delivery stays explicit: the share sheet works on any
//! network, or none.
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
                "Like a party invitation:\n\n1. You send this invitation. It grants no access on its own.\n2. They RSVP, which sends you their identity.\n3. You confirm it is really them — and you are connected.\n\nSend the card however you like — Messages, Mail, AirDrop. The invitation expires in 30 minutes.",
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
                    "They're on your list — you are connected",
                    "Nothing else is needed from you. The card in the share sheet is optional: it introduces them to your other members, so send it if you want them to know each other. Closing the share sheet changes nothing.",
                );
            } else {
                native::notice(
                    "RSVP ready to send — not connected yet",
                    "You have joined their Mesh on your side. Send this RSVP back to them: once they confirm it, you are connected and there is nothing further to do.",
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
                    &format!("Invitation from: {}\n{} people already on their list.\n\nRSVP if you are expecting this — a name is not proof, and only you know whether you asked for it.\n\nIt adds this one person to your Mesh, nobody else, and sends them your identity. They still have to confirm you before anything connects. Your serving and your existing connections stay as they are.", invite.inviter(), invite.member_count()),
                    "RSVP",
                ) { return Ok(true); }
                let next = invitation::accept(owner, &self.settings, bytes, now()?)?;
                // Accepting joins this Mesh on our side, so the runtime has to
                // restart with the inviter admitted before the reply is shared.
                self.offer_membership_card = true;
                self.queue_settings(next);
            }
            Some("acceptance") => {
                // Verifies the reply against the invitation; the code itself is not shown.
                let (member, _) = invitation::matching_code(bytes, time)?;
                let Some(allow) = native::decision(
                    "Is this really them?",
                    &format!("Someone has RSVP'd to your invitation.\n\nIdentity: {member}\n\nConfirm only if you are expecting this — a name is not proof, and only you know whether you asked them. Confirming puts this exact identity on your list and connects you; nothing further is needed from either of you. Mesh restarts itself, which takes a moment."),
                    "Confirm",
                ) else { return Ok(true); };
                let next =
                    invitation::decide_acceptance(owner, &self.settings, bytes, allow, now()?)?;
                if allow {
                    self.offer_membership_card = true;
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
                    "Their members list has been added",
                    "Their confirmation checked out. You were already connected to them; this adds the other people on their Mesh. Mesh is restarting to pick it up.",
                );
            }
            _ => return Err("Unsupported membership file".into()),
        }
        Ok(true)
    }
}

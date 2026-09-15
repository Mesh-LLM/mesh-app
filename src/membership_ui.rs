//! Two cards and nothing else: the inviter sends an invitation, the guest
//! replies. Accepting the invitation joins the guest to the inviter alone;
//! confirming the reply admits that exact identity on the inviter's side,
//! which is what connects them. File delivery stays explicit: the share sheet
//! works on any network, or none.
use crate::{native, App};
use mesh_tray::{identity, invitation, settings, text_card};
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
                "The invitation is copied to your clipboard. Paste it to them in any chat. They send a reply back; paste that in here to finish. Expires in 30 minutes.",
                "Create invitation",
            ) {
                return Ok(());
            }
            let owner = identity::ensure(&settings::mesh_profile()?)?;
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
            native::copy_text(&text_card::encode(&bytes))?;
            native::notice(
                "Invitation copied",
                "Paste it to them in any chat, mail or note.",
            );
            Ok::<(), String>(())
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
                "Nothing to send yet. Invite someone, or paste an invitation and join first.",
            )?;
            let value: serde_json::Value =
                serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            if value["mesh_pool_file"] == "approval" {
                let owner = identity::ensure(&settings::mesh_profile()?)?;
                let pid = self
                    .child
                    .as_ref()
                    .ok_or("Private runtime must be ready before sharing approval")?
                    .id();
                crate::status::private_invite(self.settings.console_port, pid, &owner.owner_id())?;
            }
            native::copy_text(&text_card::encode(bytes))?;
            native::notice(
                "Reply copied",
                "Paste it back to the person who invited you. Once they confirm it, you are connected.",
            );
            Ok::<(), String>(())
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
                    &format!("Join {}'s Mesh?", invite.inviter()),
                    "Your reply is copied to your clipboard when you join. Paste it back to them and they can connect you.",
                    "Join",
                ) {
                    return Ok(true);
                }
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
                    "Connect this person?",
                    &format!(
                        "Identity: {member}\n\nConfirm only if you asked them to join. A name is not proof."
                    ),
                    "Confirm",
                ) else {
                    return Ok(true);
                };
                let next =
                    invitation::decide_acceptance(owner, &self.settings, bytes, allow, now()?)?;
                if allow {
                    // Confirming completes the connection on both sides. There is
                    // no third card to hand over and nothing else to do.
                    self.queue_settings(next);
                    native::notice("Connected", "They are on your Mesh now.");
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
                    "Their members added",
                    "You were already connected; this adds the other people on their Mesh.",
                );
            }
            _ => return Err("Unsupported membership file".into()),
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {}

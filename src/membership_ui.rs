//! Native Invitation → RSVP → connected journey, in party terms. Two cards:
//! the inviter sends an invitation, and the guest RSVPs. Accepting the
//! invitation joins the guest to the inviter alone; confirming the RSVP admits
//! that exact identity on the inviter's side, which is what connects them. A
//! third card exists and is now optional -- it introduces the inviter's other
//! members. File delivery stays explicit: the share sheet works on any
//! network, or none.
use crate::{native, App};
use mesh_tray::{identity, invitation, text_card};
fn now() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis()
        .try_into()
        .map_err(|_| "Invalid clock".into())
}
/// Confirming an RSVP completes the connection on both sides, so the optional
/// third card is only worth offering when somebody else is already on the list
/// for the new member to be introduced to. Offered on a first connection it
/// reads like a required step and makes a two-step exchange feel like three.
fn should_introduce_other_members(admitted: &[String]) -> bool {
    admitted.len() > 1
}

impl App {
    pub(crate) fn invite_member(&mut self) {
        let result = (|| {
            if self.stopping.is_some() || self.pending_settings.is_some() {
                return Err("Wait for Mesh to finish restarting".into());
            }
            if !native::confirm(
                "Invite someone to your Mesh",
                "The invitation is copied to your clipboard — paste it to them however you like. Then paste the RSVP they send back. Expires in 30 minutes.",
                "Create invitation",
            ) {
                return Ok(());
            }
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
            native::copy_text(&text_card::encode(bytes))?;
            if invitation::card_kind(bytes) == Some("confirmation") {
                native::notice(
                    "Connected",
                    "Optional: paste the copied card to them if you want them to know your other members.",
                );
            } else {
                native::notice(
                    "RSVP copied",
                    "Send it back to the friend who invited you — any chat, mail or note. Once they confirm it, you are connected.",
                );
            }
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
                    "RSVP to this invitation?",
                    &format!(
                        "From: {}\n\nYour RSVP is copied to your clipboard when you accept — send it back to them and they can connect you.",
                        invite.inviter()
                    ),
                    "RSVP",
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
                    "Is this really them?",
                    &format!(
                        "Identity: {member}\n\nA name is not proof — confirm only if you asked this person to join. Confirming connects you."
                    ),
                    "Confirm",
                ) else {
                    return Ok(true);
                };
                let next =
                    invitation::decide_acceptance(owner, &self.settings, bytes, allow, now()?)?;
                if allow {
                    // Confirming completes the connection on both sides, so the
                    // third card is only worth offering when there is somebody
                    // else on the list for them to be introduced to. With one
                    // other person it is noise that reads like a required step.
                    self.offer_membership_card =
                        should_introduce_other_members(&next.admitted_owners);
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
mod tests {
    use super::should_introduce_other_members;

    #[test]
    fn first_connection_is_two_steps_and_later_ones_can_introduce() {
        assert!(!should_introduce_other_members(&[]));
        assert!(!should_introduce_other_members(&["jo".to_string()]));
        assert!(should_introduce_other_members(&[
            "jo".to_string(),
            "sam".to_string()
        ]));
    }
}

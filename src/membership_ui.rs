//! Native Invite → Accept & reply → Verify & allow journey. File delivery is
//! explicit until a standalone transport can propagate final grants to members.
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
            if !native::confirm("Invite a member", "Send this invitation to your friend. It grants no access. When they accept and reply, compare the matching code with them and Allow. After admission they can invite others onward.\n\nThis preview uses files for replies and final approvals; automatic delivery is not implemented. Invitations expire in 30 minutes.", "Create invitation") { return Ok(()); }
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
            let bytes = self
                .settings
                .membership_receipt
                .as_ref()
                .ok_or("No reply or approval to share yet")?;
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
            if let Ok((_, code)) = invitation::matching_code(bytes, now()?) {
                native::notice("Compare this code with your friend", &format!("{code}\n\nSend your reply, then compare this code through your known conversation. Wait for their final approval file. You have not joined yet."));
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
                if !native::confirm("Accept and reply to this invitation?", &format!("Inviter identity: {}\n{} members in their signed roster.\n\nThis sends your identity, not permission to join. Compare the matching code with your friend; they must Allow and return the final approval. Existing serving and connections stay unchanged.", invite.inviter(), invite.member_count()), "Accept & reply") { return Ok(true); }
                let next = invitation::accept(owner, &self.settings, bytes, now()?)?;
                next.save(&self.root)?;
                self.settings = next;
                self.share_membership();
            }
            Some("acceptance") => {
                let (member, code) = invitation::matching_code(bytes, time)?;
                let Some(allow) = native::decision("Confirm this is your friend", &format!("Identity: {member}\nMatching code: {code}\n\nCompare this code over your known conversation. A claimed name is not proof. Allow binds this identity to your invitation. After restarting, share the final approval through Members → Share reply or approval. Existing members can apply that approval without further pairwise confirmation."), "Allow & connect") else { return Ok(true); };
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
            }
            _ => return Err("Unsupported membership file".into()),
        }
        Ok(true)
    }
}

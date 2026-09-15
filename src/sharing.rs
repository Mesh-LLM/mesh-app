//! Native presentation delegates all consent changes to the common controller.
use crate::{consent, identity, native, App};
use mesh_tray::{exchange, share_file};
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
        identity::ensure(&crate::settings::mesh_profile()?)
    }
    pub(crate) fn share_request(&mut self) {
        let result = (|| {
            self.share_ready()?;
            if !native::confirm(
                "Request to join a private Mesh?",
                "Send this to someone you know. Their reply still needs your approval.",
                "Share request",
            ) {
                return Ok(());
            }
            let owner = self.owner()?;
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
            next.save(&self.root)?;
            self.settings = next;
            self.native
                .share(file, &self.ui.as_ref().ok_or("Tray is not ready")?._tray)
        })();
        if let Err(e) = result {
            native::notice("Could not share request", &e);
        }
    }
    /// Read whatever they sent from the clipboard. The card says which leg of
    /// the journey it is, so the user never has to.
    pub(crate) fn paste_card(&mut self) {
        let Some(text) = native::prompt_card(
            "Paste what they sent",
            "Paste the invitation or reply here. It starts with MESH1.",
            "Continue",
        ) else {
            return;
        };
        match mesh_tray::text_card::decode(&text) {
            Ok(bytes) => self.review_card(bytes),
            Err(e) => native::notice("That is not a Mesh card", &e),
        }
    }
    pub(crate) fn review_card(&mut self, bytes: Vec<u8>) {
        let result = (|| {
            self.share_ready()?;
            let owner = self.owner()?;
            let time = now()?;
            if self.review_membership_file(&bytes, &owner, time)? {
                return Ok(());
            }
            let generation = self.settings.exchange.generation();
            if let Ok(request) = exchange::verify_request(&bytes, &owner.owner_id(), time) {
                let Some(approve) = native::decision(
                    "Allow this person on your private Mesh?",
                    &format!(
                        "Claimed name: {}\nIdentity: {}\n\nCheck this identity through your known conversation. This is the legacy request/reply exchange. Use Members → Invite for transitive membership. Once Mesh is ready, the share picker opens for your reply. If delivery fails, use Share approved reply to retry.",
                        request.claimed_name(),
                        request.owner_id()
                    ),
                    "Allow & reply",
                ) else {
                    return Ok(());
                };
                let next = consent::decide_request(
                    &self.settings,
                    &bytes,
                    &owner.owner_id(),
                    generation,
                    approve,
                    now()?,
                )?;
                if approve {
                    self.queue_settings(next);
                } else {
                    next.save(&self.root)?;
                    self.settings = next;
                }
            } else {
                let response = self.settings.exchange.verify(&owner, &bytes, time)?;
                let Some(approve) = native::decision(
                    "Join this private Mesh?",
                    &format!(
                        "Identity: {}\n\nConfirm this is the person you requested. Join allows them on this node and switches this app to their private Mesh. Decline discards this reply permanently.",
                        response.owner_id()
                    ),
                    "Join",
                ) else {
                    return Ok(());
                };
                let next = consent::decide_response(&self.settings, &response, approve, now()?)?;
                if approve {
                    self.queue_settings(next);
                } else {
                    next.save(&self.root)?;
                    self.settings = next;
                }
            }
            Ok::<(), String>(())
        })();
        if let Err(e) = result {
            native::notice("Could not apply Mesh file", &e);
        }
    }
    pub(crate) fn share_reply(&mut self) {
        let result = (|| {
            self.share_ready()?;
            let owner = self.owner()?;
            let time = now()?;
            let ready = self
                .settings
                .replies
                .iter()
                .filter_map(|reply| reply.verify(&self.settings, &owner.owner_id(), time).ok())
                .collect::<Vec<_>>();
            if ready.is_empty() {
                return Err(
                    "No current approved reply is waiting. Open a friend's request first.".into(),
                );
            }
            // On automatic offer the latest approval is first. Keep earlier retries
            // durable instead of silently overwriting an undelivered grant response.
            let request = if ready.len() == 1 {
                ready.into_iter().next().unwrap()
            } else {
                let mut selected = None;
                for request in ready.into_iter().rev() {
                    if native::confirm(
                        "Share this approved reply?",
                        &format!(
                            "{}\nIdentity: {}\n\nCancel skips to the next pending reply.",
                            request.claimed_name(),
                            request.owner_id()
                        ),
                        "Share reply",
                    ) {
                        selected = Some(request);
                        break;
                    }
                }
                let Some(request) = selected else {
                    return Ok(());
                };
                request
            };
            // Never use the pre-restart snapshot. PID and owner are checked in the status adapter.
            let pid = self
                .child
                .as_ref()
                .ok_or("Mesh is not running. Retry startup before sharing.")?
                .id();
            let invite =
                crate::status::private_invite(self.settings.console_port, pid, &owner.owner_id())?;
            let bytes = exchange::seal_response(&owner, &request, &invite, now()?)?;
            let file = share_file::stage(&bytes)?;
            self.native
                .share(file, &self.ui.as_ref().ok_or("Tray is not ready")?._tray)
        })();
        if let Err(e) = result {
            native::notice("Could not share reply", &e);
        }
    }
    pub(crate) fn cancel_requests(&mut self) {
        let result = (|| {
            self.share_ready()?;
            if !native::confirm(
                "Cancel pending requests and replies?",
                "Files already sent cannot be recalled. People already on your list stay.",
                "Cancel pending",
            ) {
                return Ok(());
            }
            let next = consent::cancel(&self.settings)?;
            next.save(&self.root)?;
            self.settings = next;
            Ok::<(), String>(())
        })();
        if let Err(e) = result {
            native::notice("Could not cancel", &e);
        }
    }
}

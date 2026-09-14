//! Platform-independent consent transitions. Candidates are saved only with the
//! owned runtime stopped; verification alone never mutates persisted grants.
use crate::{
    exchange::{self, VerifiedRequest, VerifiedResponse},
    settings::{Connection, Settings},
};
use serde::{Deserialize, Serialize};

/// Persist the signed request, not an unchecked recipient key or plaintext invite.
/// A response can be retried after share cancellation or application restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    request: Vec<u8>,
    generation: u64,
    local_owner: String,
}
impl Reply {
    pub fn verify(
        &self,
        settings: &Settings,
        owner: &str,
        now: u64,
    ) -> Result<VerifiedRequest, String> {
        if self.generation != settings.exchange.generation() || self.local_owner != owner {
            return Err("Reply is no longer current for this profile.".into());
        }
        private(settings)?;
        let request = exchange::verify_request(&self.request, owner, now)?;
        if !settings
            .admitted_owners
            .iter()
            .any(|id| id == request.owner_id())
        {
            return Err("This person is no longer allowed.".into());
        }
        Ok(request)
    }
}
fn private(settings: &Settings) -> Result<(), String> {
    if !matches!(settings.connection, Connection::Private { .. }) {
        return Err("Choose Private before allowing someone to join.".into());
    }
    Ok(())
}
fn add(settings: &mut Settings, owner: &str) {
    if !settings.admitted_owners.iter().any(|id| id == owner) {
        settings.admitted_owners.push(owner.into());
    }
}
pub fn decide_request(
    current: &Settings,
    bytes: &[u8],
    local_owner: &str,
    generation: u64,
    approve: bool,
    now: u64,
) -> Result<Settings, String> {
    private(current)?;
    let request = exchange::verify_request(bytes, local_owner, now)?;
    if request.owner_id() == local_owner {
        return Err("This is your own request.".into());
    }
    let mut next = current.clone();
    next.exchange.resolve_request(&request, generation, now)?;
    if approve {
        add(&mut next, request.owner_id());
        next.owner_names
            .insert(request.owner_id().into(), request.claimed_name().into());
        next.replies
            .retain(|reply| reply.verify(current, local_owner, now).is_ok());
        if next.replies.len() >= 32 {
            return Err(
                "Too many pending replies. Cancel pending replies before approving more.".into(),
            );
        }
        next.replies.push(Reply {
            request: bytes.to_vec(),
            generation,
            local_owner: local_owner.into(),
        });
    }
    next.validate()?;
    Ok(next)
}
pub fn decide_response(
    current: &Settings,
    response: &VerifiedResponse,
    approve: bool,
    now: u64,
) -> Result<Settings, String> {
    let mut next = current.clone();
    next.exchange.consume(response, now)?;
    if approve {
        add(&mut next, response.owner_id());
        next.accept_seed(response.invite())?;
        // Joining changes the profile's connection; invalidate other open dialogs/replies.
        next.exchange.invalidate()?;
        next.replies.clear();
    }
    next.validate()?;
    Ok(next)
}
pub fn remove(current: &Settings, owner: &str, generation: u64) -> Result<Settings, String> {
    if generation != current.exchange.generation()
        || !current.admitted_owners.iter().any(|id| id == owner)
    {
        return Err("The allowed list changed. Reopen it before removing someone.".into());
    }
    let mut next = current.clone();
    next.admitted_owners.retain(|id| id != owner);
    next.owner_names.remove(owner);
    next.exchange.invalidate()?;
    next.replies.clear();
    next.validate()?;
    Ok(next)
}
pub fn cancel(current: &Settings) -> Result<Settings, String> {
    let mut next = current.clone();
    next.exchange.invalidate()?;
    next.replies.clear();
    next.issued_membership_invitations.clear();
    next.pending_membership_acceptance = None;
    next.membership_receipt = None;
    Ok(next)
}

#[cfg(test)]
mod tests;

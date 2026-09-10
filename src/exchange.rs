//! Bounded file handoffs over Mesh's existing owner keys and encrypted envelopes.
//! Verification is deliberately separate from consent and runtime application.
use ed25519_dalek::{Signature, VerifyingKey};
use mesh_llm_identity::{
    open_message, owner_id_from_verifying_key, seal_message, OwnerKeypair, SignedEncryptedEnvelope,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"mesh-tray-owner-request-v1\0";
const RESPONSE: &str = "mesh-tray.join-response.v1";
const LIFETIME: u64 = 30 * 60 * 1000;
pub const MAX_FILE_BYTES: usize = 512 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestFile {
    body: String,
    signature: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestBody {
    version: u32,
    purpose: String,
    id: String,
    issued: u64,
    expires: u64,
    signing_key: String,
    box_key: String,
    name: String,
    intended_owner: Option<String>,
}

/// Local record only: possession of a serialized record is not remote authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingRequest {
    id: String,
    digest: String,
    expires: u64,
    expected_owner: Option<String>,
    requester: String,
    generation: u64,
}

/// Only the verifier constructs this value. A name is a signed self-claim.
pub struct VerifiedRequest {
    body: RequestBody,
    digest: String,
    owner: String,
    box_key: crypto_box::PublicKey,
}
impl VerifiedRequest {
    pub fn owner_id(&self) -> &str {
        &self.owner
    }
    pub fn claimed_name(&self) -> &str {
        &self.body.name
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseBody {
    request_id: String,
    digest: String,
    expires: u64,
    invite: String,
}

/// Not an admission grant. UI must obtain consent then persist/consume the
/// matching request before applying this owner and invite to the stopped child.
/// Verified identity and invitation cannot be replaced by the native caller:
/// ```compile_fail
/// fn replace_owner(response: &mut mesh_tray::exchange::VerifiedResponse) {
///     response.owner_id = String::from("another owner");
/// }
/// ```
/// ```compile_fail
/// fn replace_invite(response: &mut mesh_tray::exchange::VerifiedResponse) {
///     response.invite = String::from("another invite");
/// }
/// ```
pub struct VerifiedResponse {
    owner_id: String,
    invite: String,
    request_id: String,
    generation: u64,
    expires: u64,
}

impl VerifiedResponse {
    pub fn owner_id(&self) -> &str {
        &self.owner_id
    }

    pub fn invite(&self) -> &str {
        &self.invite
    }
}

fn hex32(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid identity or request key".into());
    }
    hex::decode(value)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "Invalid key length".into())
}
fn lifetime(issued: u64, expires: u64, now: u64) -> Result<(), String> {
    if issued > now || expires <= now || expires <= issued || expires - issued > LIFETIME {
        return Err("Request expired or has invalid dates".into());
    }
    Ok(())
}
fn signed_bytes(body: &str) -> Vec<u8> {
    [DOMAIN, body.as_bytes()].concat()
}
fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Mesh file is too large".into());
    }
    serde_json::from_slice(bytes).map_err(|e| format!("Invalid Mesh file: {e}"))
}

pub fn create_request(
    owner: &OwnerKeypair,
    name: &str,
    intended_owner: Option<String>,
    generation: u64,
    now: u64,
) -> Result<(Vec<u8>, PendingRequest), String> {
    if name.trim().is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err("Choose a short name without control characters".into());
    }
    if let Some(id) = &intended_owner {
        hex32(id)?;
    }
    let mut nonce = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let expires = now.checked_add(LIFETIME).ok_or("Invalid time")?;
    let body = RequestBody {
        version: 1,
        purpose: "mesh-tray.join-request.v1".into(),
        id: hex::encode(nonce),
        issued: now,
        expires,
        signing_key: hex::encode(owner.verifying_key().as_bytes()),
        box_key: hex::encode(owner.encryption_public_key().as_bytes()),
        name: name.into(),
        intended_owner: intended_owner.clone(),
    };
    let body = serde_json::to_string(&body).map_err(|e| e.to_string())?;
    let pending = PendingRequest {
        id: hex::encode(nonce),
        digest: hex::encode(Sha256::digest(body.as_bytes())),
        expires,
        expected_owner: intended_owner,
        requester: owner.owner_id(),
        generation,
    };
    let signature = hex::encode(owner.sign_bytes(&signed_bytes(&body)));
    let file = serde_json::to_vec(&RequestFile { body, signature }).map_err(|e| e.to_string())?;
    Ok((file, pending))
}

pub fn verify_request(
    bytes: &[u8],
    local_owner: &str,
    now: u64,
) -> Result<VerifiedRequest, String> {
    let file: RequestFile = parse(bytes)?;
    if file.body.len() > 4096 || file.signature.len() != 128 {
        return Err("Invalid request size".into());
    }
    let body: RequestBody = parse(file.body.as_bytes())?;
    if body.version != 1 || body.purpose != "mesh-tray.join-request.v1" {
        return Err("Unsupported Mesh request".into());
    }
    lifetime(body.issued, body.expires, now)?;
    hex32(&body.id)?;
    if body.name.trim().is_empty()
        || body.name.len() > 128
        || body.name.chars().any(char::is_control)
    {
        return Err("Invalid requester name".into());
    }
    if let Some(intended) = &body.intended_owner {
        hex32(intended)?;
        if intended != local_owner {
            return Err("Request is addressed to another owner".into());
        }
    }
    let key = VerifyingKey::from_bytes(&hex32(&body.signing_key)?).map_err(|e| e.to_string())?;
    let signature: [u8; 64] = hex::decode(&file.signature)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "Invalid signature length")?;
    key.verify_strict(
        &signed_bytes(&file.body),
        &Signature::from_bytes(&signature),
    )
    .map_err(|_| "Request signature is invalid")?;
    let box_key = crypto_box::PublicKey::from(hex32(&body.box_key)?);
    Ok(VerifiedRequest {
        body,
        digest: hex::encode(Sha256::digest(file.body.as_bytes())),
        owner: owner_id_from_verifying_key(&key),
        box_key,
    })
}

/// Only call after native consent and effective local admission. This operation
/// merely constructs a recipient-bound file; it never changes a trust list.
pub fn seal_response(
    owner: &OwnerKeypair,
    request: &VerifiedRequest,
    invite: &str,
    now: u64,
) -> Result<Vec<u8>, String> {
    lifetime(request.body.issued, request.body.expires, now)?;
    if request
        .body
        .intended_owner
        .as_ref()
        .is_some_and(|id| id != &owner.owner_id())
    {
        return Err("Request is addressed to another owner".into());
    }
    validate_invite(invite)?;
    let body = ResponseBody {
        request_id: request.body.id.clone(),
        digest: request.digest.clone(),
        expires: request.body.expires,
        invite: invite.into(),
    };
    let payload = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let envelope = seal_message(owner, &request.box_key, RESPONSE, &payload, now)
        .map_err(|e| e.to_string())?;
    serde_json::to_vec(&envelope).map_err(|e| e.to_string())
}

fn validate_invite(invite: &str) -> Result<(), String> {
    if invite.is_empty()
        || invite.len() > 32768
        || !invite
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Invalid Mesh invitation".into());
    }
    Ok(())
}

pub fn verify_response(
    owner: &OwnerKeypair,
    bytes: &[u8],
    pending: &PendingRequest,
    generation: u64,
    now: u64,
) -> Result<VerifiedResponse, String> {
    if pending.generation != generation
        || pending.requester != owner.owner_id()
        || pending.expires <= now
    {
        return Err("Request is no longer pending for this profile".into());
    }
    let envelope: SignedEncryptedEnvelope = parse(bytes)?;
    let opened = open_message(owner, &envelope).map_err(|e| e.to_string())?;
    if opened.message_type != RESPONSE
        || pending
            .expected_owner
            .as_ref()
            .is_some_and(|id| id != &opened.sender_owner_id)
    {
        return Err("Unexpected Mesh response sender or purpose".into());
    }
    let body: ResponseBody = parse(&opened.payload)?;
    lifetime(opened.timestamp_unix_ms, body.expires, now)?;
    if body.request_id != pending.id
        || body.digest != pending.digest
        || body.expires > pending.expires
    {
        return Err("Response does not match the outstanding request".into());
    }
    validate_invite(&body.invite)?;
    Ok(VerifiedResponse {
        owner_id: opened.sender_owner_id,
        invite: body.invite,
        request_id: pending.id.clone(),
        generation,
        expires: body.expires,
    })
}

/// Keep this state in the same atomic preferences transaction as admitted owners.
/// Epoch changes invalidate open dialogs as well as persisted pending requests.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExchangeState {
    generation: u64,
    pending: Vec<PendingRequest>,
    seen_requests: Vec<(String, u64)>,
}
impl ExchangeState {
    /// Record a host-side approval or decline together with the list update.
    /// Retain digests until expiry even across removal/profile invalidation.
    pub fn resolve_request(
        &mut self,
        request: &VerifiedRequest,
        dialog_generation: u64,
        now: u64,
    ) -> Result<(), String> {
        lifetime(request.body.issued, request.body.expires, now)?;
        self.seen_requests.retain(|(_, expiry)| *expiry > now);
        if dialog_generation != self.generation
            || self.seen_requests.len() >= 1024
            || self
                .seen_requests
                .iter()
                .any(|(digest, _)| digest == &request.digest)
        {
            return Err("Request already resolved or approval is stale".into());
        }
        self.seen_requests
            .push((request.digest.clone(), request.body.expires));
        Ok(())
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn add_pending(&mut self, pending: PendingRequest, now: u64) -> Result<(), String> {
        self.pending.retain(|p| p.expires > now);
        if pending.generation != self.generation
            || pending.expires <= now
            || self.pending.len() >= 32
            || self.pending.iter().any(|p| p.id == pending.id)
        {
            return Err("Cannot add this pending request".into());
        }
        self.pending.push(pending);
        Ok(())
    }
    pub fn verify(
        &self,
        owner: &OwnerKeypair,
        bytes: &[u8],
        now: u64,
    ) -> Result<VerifiedResponse, String> {
        self.pending
            .iter()
            .find_map(|pending| verify_response(owner, bytes, pending, self.generation, now).ok())
            .ok_or("No matching pending Mesh request".into())
    }
    /// Consent must precede consumption. Persist the changed state and grant
    /// together before restarting; a failed save must not apply either change.
    pub fn consume(&mut self, response: &VerifiedResponse, now: u64) -> Result<(), String> {
        if response.generation != self.generation || response.expires <= now {
            return Err("Approval is stale; reopen the request".into());
        }
        let index = self
            .pending
            .iter()
            .position(|p| p.id == response.request_id)
            .ok_or("Request already consumed or cancelled")?;
        self.pending.remove(index);
        Ok(())
    }
    /// Removal, cancellation or profile switch invalidates all outstanding files.
    pub fn invalidate(&mut self) -> Result<(), String> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("Exchange generation exhausted")?;
        self.pending.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests;

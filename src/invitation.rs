//! Identity-bound Invite → Accept & reply → Verify & allow. Invitations and
//! acceptances grant nothing. The inviter signs the exact recipient only after
//! explicit confirmation; trusted members apply that grant transitively.
//! File delivery remains explicit, not an automatic admission transport.
use crate::settings::{Connection, Settings};
use ed25519_dalek::{Signature, VerifyingKey};
use mesh_llm_identity::{owner_id_from_verifying_key, OwnerKeypair};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"mesh-tray-pool-membership-v1\0";
const LIFETIME: u64 = 30 * 60 * 1000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Signed {
    body: String,
    key: String,
    signature: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Offer {
    purpose: String,
    nonce: String,
    issued: u64,
    expires: u64,
    members: Vec<String>,
    seeds: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Acceptance {
    purpose: String,
    invitation_digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    purpose: String,
    acceptance_digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mesh_pool_file", rename_all = "snake_case", deny_unknown_fields)]
enum File {
    Approval {
        invitation: Signed,
        acceptance: Signed,
        approval: Signed,
    },
    Invitation {
        invitation: Signed,
    },
    Acceptance {
        invitation: Signed,
        acceptance: Signed,
    },
}

/// Which of the three cards this file is, in the words the tray shows a human.
/// Presentation only: it neither verifies signatures nor grants anything.
pub fn card_kind(bytes: &[u8]) -> Option<&'static str> {
    match parse(bytes).ok()? {
        File::Invitation { .. } => Some("invitation"),
        File::Acceptance { .. } => Some("RSVP"),
        File::Approval { .. } => Some("confirmation"),
    }
}

pub struct VerifiedInvitation {
    signed: Signed,
    offer: Offer,
    inviter: String,
}
impl VerifiedInvitation {
    pub fn inviter(&self) -> &str {
        &self.inviter
    }
    pub fn member_count(&self) -> usize {
        self.offer.members.len()
    }
}
fn sign(owner: &OwnerKeypair, body: &impl Serialize) -> Result<Signed, String> {
    let body = serde_json::to_string(body).map_err(|e| e.to_string())?;
    let signature = hex::encode(owner.sign_bytes(&[DOMAIN, body.as_bytes()].concat()));
    Ok(Signed {
        body,
        key: hex::encode(owner.verifying_key().as_bytes()),
        signature,
    })
}
fn verify<T: serde::de::DeserializeOwned>(signed: &Signed) -> Result<(String, T), String> {
    let key: [u8; 32] = hex::decode(&signed.key)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "Invalid invitation key")?;
    let signature: [u8; 64] = hex::decode(&signed.signature)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "Invalid invitation signature")?;
    let key = VerifyingKey::from_bytes(&key).map_err(|e| e.to_string())?;
    key.verify_strict(
        &[DOMAIN, signed.body.as_bytes()].concat(),
        &Signature::from_bytes(&signature),
    )
    .map_err(|_| "Invitation signature is invalid")?;
    Ok((
        owner_id_from_verifying_key(&key),
        serde_json::from_str(&signed.body).map_err(|e| e.to_string())?,
    ))
}
fn parse(bytes: &[u8]) -> Result<File, String> {
    if bytes.len() > crate::exchange::MAX_FILE_BYTES {
        return Err("Mesh file too large".into());
    }
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}
fn serialize(file: &File) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(file).map_err(|e| e.to_string())?;
    if bytes.len() > crate::exchange::MAX_FILE_BYTES {
        return Err("Mesh file too large".into());
    }
    Ok(bytes)
}
fn check(invitation: Signed, now: u64) -> Result<VerifiedInvitation, String> {
    let (inviter, offer): (_, Offer) = verify(&invitation)?;
    if offer.purpose != "mesh-tray.pool-invite.v1"
        || offer.issued > now
        || offer.expires <= now
        || offer.expires <= offer.issued
        || offer.expires - offer.issued > LIFETIME
    {
        return Err("Invitation expired or has invalid dates/type".into());
    }
    if offer.nonce.len() != 64 || hex::decode(&offer.nonce).is_err() {
        return Err("Invalid invitation nonce".into());
    }
    crate::admission::validate_owners(&offer.members)?;
    if !offer.members.contains(&inviter) || offer.seeds.is_empty() || offer.seeds.len() > 32 {
        return Err("Invalid invitation membership/seeds".into());
    }
    for seed in &offer.seeds {
        crate::settings::validate_invite(seed)?;
    }
    Ok(VerifiedInvitation {
        signed: invitation,
        offer,
        inviter,
    })
}

pub fn create(
    owner: &OwnerKeypair,
    settings: &Settings,
    seed: &str,
    now: u64,
) -> Result<Vec<u8>, String> {
    if !matches!(settings.connection, Connection::Private { .. }) {
        return Err("Choose Private before inviting a member".into());
    }
    let mut members = settings.admitted_owners.clone();
    if !members.contains(&owner.owner_id()) {
        members.push(owner.owner_id());
    }
    let offer = Offer {
        purpose: "mesh-tray.pool-invite.v1".into(),
        nonce: hex::encode(rand::random::<[u8; 32]>()),
        issued: now,
        expires: now.checked_add(LIFETIME).ok_or("Invalid clock")?,
        members,
        seeds: vec![seed.into()],
    };
    let invitation = sign(owner, &offer)?;
    check(invitation.clone(), now)?;
    serialize(&File::Invitation { invitation })
}
pub fn inspect(bytes: &[u8], now: u64) -> Result<VerifiedInvitation, String> {
    match parse(bytes)? {
        File::Invitation { invitation } => check(invitation, now),
        _ => Err("Not a pool invitation".into()),
    }
}
fn add_members(next: &mut Settings, members: impl IntoIterator<Item = String>, local: &str) {
    for member in members {
        if member != local && !next.admitted_owners.contains(&member) {
            next.admitted_owners.push(member);
        }
    }
}
/// Caller obtains explicit native Accept consent before persisting this candidate.
pub fn accept(
    owner: &OwnerKeypair,
    settings: &Settings,
    bytes: &[u8],
    now: u64,
) -> Result<Settings, String> {
    let invitation = inspect(bytes, now)?;
    if invitation.inviter == owner.owner_id() {
        return Err("This is your own invitation".into());
    }
    let acceptance = sign(
        owner,
        &Acceptance {
            purpose: "mesh-tray.pool-accept.v1".into(),
            invitation_digest: hex::encode(Sha256::digest(invitation.signed.body.as_bytes())),
        },
    )?;
    let receipt = serialize(&File::Acceptance {
        invitation: invitation.signed,
        acceptance,
    })?;
    let mut next = settings.clone();
    // Accept only records the identity-bound reply. No grants, seeds or mode change.
    next.pending_membership_acceptance = Some(hex::encode(Sha256::digest(&receipt)));
    next.membership_receipt = Some(receipt);
    next.validate()?;
    Ok(next)
}

pub fn remember_invitation(
    settings: &Settings,
    bytes: &[u8],
    now: u64,
) -> Result<Settings, String> {
    let invite = inspect(bytes, now)?;
    let mut next = settings.clone();
    let digest = hex::encode(Sha256::digest(invite.signed.body.as_bytes()));
    if !next.issued_membership_invitations.contains(&digest) {
        next.issued_membership_invitations.push(digest);
    }
    next.validate()?;
    Ok(next)
}

fn accepted(
    invitation: Signed,
    acceptance: &Signed,
    now: u64,
) -> Result<(VerifiedInvitation, String), String> {
    let invitation = check(invitation, now)?;
    let (member, accepted): (_, Acceptance) = verify(acceptance)?;
    if accepted.purpose != "mesh-tray.pool-accept.v1"
        || accepted.invitation_digest
            != hex::encode(Sha256::digest(invitation.signed.body.as_bytes()))
    {
        return Err("Acceptance does not match invitation".into());
    }
    Ok((invitation, member))
}

/// Identity plus 80-bit transcript code, to compare over a known conversation.
/// Both parties derive it from the full signed acceptance, not a claimed name.
pub fn matching_code(bytes: &[u8], now: u64) -> Result<(String, String), String> {
    let File::Acceptance {
        invitation,
        acceptance,
    } = parse(bytes)?
    else {
        return Err("Not an acceptance".into());
    };
    let (_, member) = accepted(invitation, &acceptance, now)?;
    let canonical = serialize(&parse(bytes)?)?;
    let digest = hex::encode(Sha256::digest(canonical));
    Ok((
        member,
        digest.as_bytes()[..20]
            .chunks(4)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join("-"),
    ))
}

/// Invoked only after human out-of-band checking and explicit Allow/Decline.
/// `matching_code` derives an 80-bit code from the full signed reply, so a
/// substituted identity yields a different one. The tray does not currently
/// show it: the human is asked only whether they expect this reply. Nothing
/// here was ever enforced by the code -- only Allow/Decline is.
/// Consume the issued invitation on either decision; one response cannot admit
/// several identities. Persist the candidate with the owned runtime stopped.
pub fn decide_acceptance(
    owner: &OwnerKeypair,
    settings: &Settings,
    bytes: &[u8],
    allow: bool,
    now: u64,
) -> Result<Settings, String> {
    let File::Acceptance {
        invitation,
        acceptance,
    } = parse(bytes)?
    else {
        return Err("Not an acceptance".into());
    };
    let (invitation, member) = accepted(invitation, &acceptance, now)?;
    let digest = hex::encode(Sha256::digest(invitation.signed.body.as_bytes()));
    if invitation.inviter != owner.owner_id()
        || !settings.issued_membership_invitations.contains(&digest)
    {
        return Err("This invitation is not pending on this profile".into());
    }
    let mut next = settings.clone();
    next.issued_membership_invitations
        .retain(|saved| saved != &digest);
    if allow {
        let approval = sign(
            owner,
            &Approval {
                purpose: "mesh-tray.pool-allow.v1".into(),
                acceptance_digest: hex::encode(Sha256::digest(serialize(&File::Acceptance {
                    invitation: invitation.signed.clone(),
                    acceptance: acceptance.clone(),
                })?)),
            },
        )?;
        next.membership_receipt = Some(serialize(&File::Approval {
            invitation: invitation.signed,
            acceptance,
            approval,
        })?);
        add_members(&mut next, [member], &owner.owner_id());
    }
    next.validate()?;
    Ok(next)
}

/// Recipients require their saved acceptance; existing members require a trusted
/// inviter. Only the inviter's final signature authorizes the exact new identity.
pub fn apply_receipt(
    owner: &str,
    settings: &Settings,
    bytes: &[u8],
    now: u64,
) -> Result<Settings, String> {
    let File::Approval {
        invitation,
        acceptance,
        approval,
    } = parse(bytes)?
    else {
        return Err("Waiting for inviter approval; acceptance alone grants nothing".into());
    };
    let acceptance_file = serialize(&File::Acceptance {
        invitation: invitation.clone(),
        acceptance: acceptance.clone(),
    })?;
    let (invitation, member) = accepted(invitation, &acceptance, now)?;
    let (approver, approved): (_, Approval) = verify(&approval)?;
    let acceptance_digest = hex::encode(Sha256::digest(&acceptance_file));
    if approver != invitation.inviter
        || approved.purpose != "mesh-tray.pool-allow.v1"
        || approved.acceptance_digest != acceptance_digest
    {
        return Err("Approval does not bind this invitation and recipient".into());
    }
    if member == owner {
        if settings.pending_membership_acceptance.as_ref() != Some(&acceptance_digest) {
            return Err("No matching accepted invitation on this profile".into());
        }
    } else if !matches!(settings.connection, Connection::Private { .. })
        || !settings.admitted_owners.contains(&approver)
    {
        return Err("Approval inviter is not a member of this private Mesh".into());
    }
    let digest = hex::encode(Sha256::digest(approval.body.as_bytes()));
    if settings.applied_membership_receipts.contains(&digest) {
        return Err("Membership receipt already applied".into());
    }
    let mut next = settings.clone();
    next.applied_membership_receipts.push(digest);
    if member == owner {
        add_members(&mut next, invitation.offer.members, owner);
        for seed in invitation.offer.seeds {
            next.accept_seed(&seed)?;
        }
        next.pending_membership_acceptance = None;
    } else {
        add_members(&mut next, [member], owner);
    }
    next.membership_receipt = Some(bytes.to_vec());
    next.validate()?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn private() -> Settings {
        Settings {
            connection: Connection::Private { invite: None },
            ..Default::default()
        }
    }
    fn pair(
        a: &OwnerKeypair,
        sa: &Settings,
        b: &OwnerKeypair,
        sb: &Settings,
        seed: &str,
    ) -> (Settings, Settings) {
        let offer = create(a, sa, seed, 100).unwrap();
        let sa = remember_invitation(sa, &offer, 100).unwrap();
        let sb = accept(b, sb, &offer, 101).unwrap();
        let receipt = sb.membership_receipt.as_ref().unwrap();
        assert!(apply_receipt(&a.owner_id(), &sa, receipt, 102).is_err());
        let sa = decide_acceptance(a, &sa, receipt, true, 102).unwrap();
        let sb = apply_receipt(
            &b.owner_id(),
            &sb,
            sa.membership_receipt.as_ref().unwrap(),
            103,
        )
        .unwrap();
        (sa, sb)
    }
    #[test]
    fn card_kind_names_each_leg_of_the_journey() {
        let a = OwnerKeypair::generate();
        let b = OwnerKeypair::generate();
        let sa = private();
        let invitation = create(&a, &sa, "seed-a", 100).unwrap();
        let sa = remember_invitation(&sa, &invitation, 100).unwrap();
        let sb = accept(&b, &private(), &invitation, 101).unwrap();
        let rsvp = sb.membership_receipt.clone().unwrap();
        let sa = decide_acceptance(&a, &sa, &rsvp, true, 102).unwrap();
        let confirmation = sa.membership_receipt.clone().unwrap();
        assert_eq!(card_kind(&invitation), Some("invitation"));
        assert_eq!(card_kind(&rsvp), Some("RSVP"));
        assert_eq!(card_kind(&confirmation), Some("confirmation"));
        assert_eq!(card_kind(b"not a card"), None);
        // The card the guest shows and the card the inviter reads derive the
        // same matching code, which is the whole point of comparing them.
        assert_eq!(
            matching_code(&rsvp, 102).unwrap().1,
            matching_code(sb.membership_receipt.as_ref().unwrap(), 102)
                .unwrap()
                .1
        );
    }
    #[test]
    fn onward_pooling_requires_inviter_allow_not_every_members_approval() {
        let a = OwnerKeypair::generate();
        let b = OwnerKeypair::generate();
        let c = OwnerKeypair::generate();
        let (sa, sb) = pair(&a, &private(), &b, &private(), "seed-a");
        let (sb, sc) = pair(&b, &sb, &c, &private(), "seed-b");
        assert!(sc.admitted_owners.contains(&a.owner_id()));
        let sa = apply_receipt(
            &a.owner_id(),
            &sa,
            sb.membership_receipt.as_ref().unwrap(),
            104,
        )
        .unwrap();
        assert!(sa.admitted_owners.contains(&c.owner_id()));
        let (_, sb) = pair(&c, &sc, &b, &sb, "seed-c");
        assert_eq!(
            sb.connection,
            Connection::Private {
                invite: Some("seed-a".into())
            }
        );
        assert_eq!(sb.seeds, ["seed-c"]);
        assert_eq!(sb.args()[0], "serve");
    }
    #[test]
    fn acceptance_never_grants_and_decline_consumes_invitation() {
        let a = OwnerKeypair::generate();
        let b = OwnerKeypair::generate();
        let offer = create(&a, &private(), "seed", 100).unwrap();
        let sa = remember_invitation(&private(), &offer, 100).unwrap();
        let sb = accept(&b, &Settings::default(), &offer, 101).unwrap();
        assert!(sb.admitted_owners.is_empty());
        assert_eq!(sb.connection, Connection::Automatic);
        let reply = sb.membership_receipt.as_ref().unwrap();
        let declined = decide_acceptance(&a, &sa, reply, false, 102).unwrap();
        assert!(declined.admitted_owners.is_empty());
        assert!(decide_acceptance(&a, &declined, reply, true, 103).is_err());
        assert!(apply_receipt(&a.owner_id(), &sa, reply, 103).is_err());
        assert_eq!(matching_code(reply, 102).unwrap().0, b.owner_id());
    }
    #[test]
    fn forwarded_invitation_cannot_admit_second_identity_after_allow() {
        let a = OwnerKeypair::generate();
        let b = OwnerKeypair::generate();
        let x = OwnerKeypair::generate();
        let offer = create(&a, &private(), "seed", 100).unwrap();
        let sa = remember_invitation(&private(), &offer, 100).unwrap();
        let sb = accept(&b, &private(), &offer, 101).unwrap();
        let sx = accept(&x, &private(), &offer, 101).unwrap();
        let sa =
            decide_acceptance(&a, &sa, sb.membership_receipt.as_ref().unwrap(), true, 102).unwrap();
        assert_eq!(sa.admitted_owners, [b.owner_id()]);
        assert!(
            decide_acceptance(&a, &sa, sx.membership_receipt.as_ref().unwrap(), true, 103).is_err()
        );
        assert!(apply_receipt(
            &x.owner_id(),
            &sx,
            sa.membership_receipt.as_ref().unwrap(),
            103
        )
        .is_err());
        // Substituting a valid but different acceptance cannot reuse A's approval.
        let mut grant = parse(sa.membership_receipt.as_ref().unwrap()).unwrap();
        let File::Acceptance {
            acceptance: other, ..
        } = parse(sx.membership_receipt.as_ref().unwrap()).unwrap()
        else {
            panic!()
        };
        if let File::Approval { acceptance, .. } = &mut grant {
            *acceptance = other;
        }
        assert!(apply_receipt(&x.owner_id(), &sx, &serialize(&grant).unwrap(), 103).is_err());
    }

    #[test]
    fn persisted_exchange_survives_restart_without_early_admission() {
        let a = OwnerKeypair::generate();
        let b = OwnerKeypair::generate();
        let ra = tempfile::tempdir().unwrap();
        let rb = tempfile::tempdir().unwrap();
        let offer = create(&a, &private(), "seed", 100).unwrap();
        remember_invitation(&private(), &offer, 100)
            .unwrap()
            .save(ra.path())
            .unwrap();
        accept(&b, &private(), &offer, 101)
            .unwrap()
            .save(rb.path())
            .unwrap();
        let sa = Settings::load(ra.path()).unwrap();
        let sb = Settings::load(rb.path()).unwrap();
        assert!(sa.admitted_owners.is_empty() && sb.admitted_owners.is_empty());
        let sa =
            decide_acceptance(&a, &sa, sb.membership_receipt.as_ref().unwrap(), true, 102).unwrap();
        sa.save(ra.path()).unwrap();
        let sa = Settings::load(ra.path()).unwrap();
        let sb = apply_receipt(
            &b.owner_id(),
            &sb,
            sa.membership_receipt.as_ref().unwrap(),
            103,
        )
        .unwrap();
        sb.save(rb.path()).unwrap();
        let sb = Settings::load(rb.path()).unwrap();
        assert_eq!(sb.admitted_owners, [a.owner_id()]);
        assert_eq!(sb.args()[0], "serve");
        assert!(apply_receipt(
            &b.owner_id(),
            &sb,
            sa.membership_receipt.as_ref().unwrap(),
            104
        )
        .is_err());
    }

    #[test]
    fn forged_replayed_expired_and_uncorrelated_approval_rejected() {
        let a = OwnerKeypair::generate();
        let b = OwnerKeypair::generate();
        let x = OwnerKeypair::generate();
        let (sa, sb) = pair(&a, &private(), &b, &private(), "seed");
        let grant = sa.membership_receipt.as_ref().unwrap();
        assert!(apply_receipt(&x.owner_id(), &private(), grant, 104).is_err());
        assert!(apply_receipt(&b.owner_id(), &sb, grant, 104).is_err());
        assert!(apply_receipt(&b.owner_id(), &private(), grant, 104).is_err());
        assert!(apply_receipt(&b.owner_id(), &sb, grant, 100 + LIFETIME).is_err());
        let mut file = parse(grant).unwrap();
        if let File::Approval { approval, .. } = &mut file {
            approval.key = hex::encode(x.verifying_key().as_bytes());
        }
        assert!(apply_receipt(&b.owner_id(), &sb, &serialize(&file).unwrap(), 104).is_err());
    }
}

use super::*;
const NOW: u64 = 1_000_000;

fn pair() -> (OwnerKeypair, OwnerKeypair, ExchangeState, Vec<u8>) {
    let a = OwnerKeypair::generate();
    let b = OwnerKeypair::generate();
    let mut state = ExchangeState::default();
    let (request, pending) = create_request(&b, "Family", Some(a.owner_id()), 0, NOW).unwrap();
    state.add_pending(pending, NOW).unwrap();
    let verified = verify_request(&request, &a.owner_id(), NOW).unwrap();
    assert_eq!(verified.owner_id(), b.owner_id());
    let response = seal_response(&a, &verified, "complete-opaque_invite", NOW).unwrap();
    (a, b, state, response)
}

#[test]
fn roundtrip_requires_pending_request_and_consumes_durably() {
    let (a, b, mut state, response) = pair();
    let verified = state.verify(&b, &response, NOW).unwrap();
    assert_eq!(verified.owner_id(), a.owner_id());
    assert_eq!(verified.invite(), "complete-opaque_invite");
    state.consume(&verified, NOW).unwrap();
    let saved = serde_json::to_vec(&state).unwrap();
    let reloaded: ExchangeState = serde_json::from_slice(&saved).unwrap();
    assert!(reloaded.verify(&b, &response, NOW).is_err());
    assert!(state.consume(&verified, NOW).is_err());
}
#[test]
fn forwarding_does_not_change_recipient() {
    let (_, _, state, response) = pair();
    assert!(state
        .verify(&OwnerKeypair::generate(), &response, NOW)
        .is_err());
}
#[test]
fn removal_invalidates_files_and_already_open_dialogs() {
    let (_, b, mut state, response) = pair();
    let verified = state.verify(&b, &response, NOW).unwrap();
    state.invalidate().unwrap();
    assert!(state.consume(&verified, NOW).is_err());
    assert!(state.verify(&b, &response, NOW).is_err());
}
#[test]
fn expired_dialog_and_response_reject() {
    let (_, b, mut state, response) = pair();
    let verified = state.verify(&b, &response, NOW).unwrap();
    assert!(state.consume(&verified, NOW + LIFETIME).is_err());
    assert!(state.verify(&b, &response, NOW + LIFETIME).is_err());
}
#[test]
fn pending_survives_restart_but_does_not_authorize_stranger() {
    let (a, b, state, response) = pair();
    let saved = serde_json::to_vec(&state).unwrap();
    let state: ExchangeState = serde_json::from_slice(&saved).unwrap();
    assert_eq!(
        state.verify(&b, &response, NOW).unwrap().owner_id(),
        a.owner_id()
    );
    let original: SignedEncryptedEnvelope = parse(&response).unwrap();
    let payload = open_message(&b, &original).unwrap().payload;
    let stranger = OwnerKeypair::generate();
    let forged = seal_message(
        &stranger,
        &b.encryption_public_key(),
        RESPONSE,
        &payload,
        NOW,
    )
    .unwrap();
    assert!(state
        .verify(&b, &serde_json::to_vec(&forged).unwrap(), NOW)
        .is_err());
}
#[test]
fn signed_fields_cannot_be_substituted() {
    let a = OwnerKeypair::generate();
    let b = OwnerKeypair::generate();
    let (request, _) = create_request(&b, "Family", Some(a.owner_id()), 0, NOW).unwrap();
    for field in ["box_key", "signing_key", "id", "name", "expires", "purpose"] {
        let mut file: RequestFile = parse(&request).unwrap();
        let mut body: serde_json::Value = serde_json::from_str(&file.body).unwrap();
        body[field] = match field {
            "name" => serde_json::json!("Someone else"),
            "expires" => serde_json::json!(NOW + 5),
            "purpose" => serde_json::json!("different"),
            _ => serde_json::json!("ab".repeat(32)),
        };
        file.body = serde_json::to_string(&body).unwrap();
        assert!(
            verify_request(&serde_json::to_vec(&file).unwrap(), &a.owner_id(), NOW).is_err(),
            "{field}"
        );
    }
}
#[test]
fn unknown_request_wrong_purpose_and_tampering_reject() {
    let (a, b, state, response) = pair();
    let mut envelope: SignedEncryptedEnvelope = parse(&response).unwrap();
    let opened = open_message(&b, &envelope).unwrap();
    let wrong_type = seal_message(
        &a,
        &b.encryption_public_key(),
        "wrong",
        &opened.payload,
        NOW,
    )
    .unwrap();
    assert!(state
        .verify(&b, &serde_json::to_vec(&wrong_type).unwrap(), NOW)
        .is_err());
    let mut body: ResponseBody = parse(&opened.payload).unwrap();
    body.digest = "ab".repeat(32);
    let unrelated = seal_message(
        &a,
        &b.encryption_public_key(),
        RESPONSE,
        &serde_json::to_vec(&body).unwrap(),
        NOW,
    )
    .unwrap();
    assert!(state
        .verify(&b, &serde_json::to_vec(&unrelated).unwrap(), NOW)
        .is_err());
    let mut ciphertext = hex::decode(&envelope.ciphertext).unwrap();
    ciphertext[0] ^= 1;
    envelope.ciphertext = hex::encode(ciphertext);
    assert!(state
        .verify(&b, &serde_json::to_vec(&envelope).unwrap(), NOW)
        .is_err());
    assert!(ExchangeState::default().verify(&b, &response, NOW).is_err());
}
#[test]
fn wrong_request_recipient_and_oversize_reject() {
    let a = OwnerKeypair::generate();
    let b = OwnerKeypair::generate();
    let (request, _) = create_request(&b, "Family", Some(a.owner_id()), 0, NOW).unwrap();
    assert!(verify_request(&request, &b.owner_id(), NOW).is_err());
    assert!(verify_request(&vec![b' '; MAX_FILE_BYTES + 1], &a.owner_id(), NOW).is_err());
    assert!(verify_request(&request, &a.owner_id(), NOW - 1).is_err());
}

#[test]
fn host_replay_stays_resolved_after_removal_and_restart() {
    let a = OwnerKeypair::generate();
    let b = OwnerKeypair::generate();
    let (file, _) = create_request(&b, "Family", None, 0, NOW).unwrap();
    let request = verify_request(&file, &a.owner_id(), NOW).unwrap();
    let mut state = ExchangeState::default();
    state.resolve_request(&request, 0, NOW).unwrap();
    state.invalidate().unwrap();
    let mut reloaded: ExchangeState =
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(reloaded
        .resolve_request(&request, reloaded.generation(), NOW)
        .is_err());
    let (fresh, _) = create_request(&b, "Family", None, 0, NOW).unwrap();
    let fresh = verify_request(&fresh, &a.owner_id(), NOW).unwrap();
    assert!(reloaded.resolve_request(&fresh, 0, NOW).is_err());
    reloaded
        .resolve_request(&fresh, reloaded.generation(), NOW)
        .unwrap();
}

#[test]
fn unpinned_authentic_response_only_verifies_without_changing_state() {
    let stranger = OwnerKeypair::generate();
    let requester = OwnerKeypair::generate();
    let mut state = ExchangeState::default();
    let (file, pending) = create_request(&requester, "Family", None, 0, NOW).unwrap();
    state.add_pending(pending, NOW).unwrap();
    let request = verify_request(&file, &stranger.owner_id(), NOW).unwrap();
    let reply = seal_response(&stranger, &request, "opaque-invite", NOW).unwrap();
    let before = serde_json::to_vec(&state).unwrap();
    let verified = state.verify(&requester, &reply, NOW).unwrap();
    assert_eq!(verified.owner_id(), stranger.owner_id());
    assert_eq!(before, serde_json::to_vec(&state).unwrap());
    // Deliberately no consume/grant operation: authenticity is not consent.
    assert!(state.verify(&requester, &reply, NOW).is_ok());
}

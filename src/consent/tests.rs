use super::*;
use mesh_llm_identity::OwnerKeypair;
fn private_settings() -> Settings {
    Settings {
        connection: Connection::Private { invite: None },
        ..Default::default()
    }
}
#[test]
fn approve_decline_and_duplicate_are_explicit_and_durable() {
    let host = OwnerKeypair::generate();
    let guest = OwnerKeypair::generate();
    let settings = private_settings();
    let (bytes, _) = exchange::create_request(&guest, "Friend", None, 0, 100).unwrap();
    let req = exchange::verify_request(&bytes, &host.owner_id(), 101).unwrap();
    assert!(settings.admitted_owners.is_empty());
    let declined = decide_request(&settings, &bytes, &host.owner_id(), 0, false, 101).unwrap();
    assert!(declined.admitted_owners.is_empty());
    assert!(decide_request(&declined, &bytes, &host.owner_id(), 0, true, 102).is_err());
    let approved = decide_request(&settings, &bytes, &host.owner_id(), 0, true, 101).unwrap();
    assert_eq!(approved.admitted_owners, vec![req.owner_id()]);
    let root = tempfile::tempdir().unwrap();
    approved.save(root.path()).unwrap();
    let reloaded = Settings::load(root.path()).unwrap();
    assert!(reloaded
        .replies
        .last()
        .unwrap()
        .verify(&reloaded, &host.owner_id(), 102)
        .is_ok());
    assert!(decide_request(&reloaded, &bytes, &host.owner_id(), 0, true, 102).is_err());
}
#[test]
fn removal_and_cancel_invalidate_stale_clicks_and_replies() {
    let h = OwnerKeypair::generate();
    let g = OwnerKeypair::generate();
    let s = private_settings();
    let (b, _) = exchange::create_request(&g, "Friend", None, 0, 100).unwrap();
    let a = decide_request(&s, &b, &h.owner_id(), 0, true, 101).unwrap();
    let reply = a.replies.last().unwrap().clone();
    let removed = remove(&a, &g.owner_id(), 0).unwrap();
    assert!(reply.verify(&removed, &h.owner_id(), 102).is_err());
    assert!(decide_request(&removed, &b, &h.owner_id(), 0, true, 102).is_err());
    assert!(decide_request(&removed, &b, &h.owner_id(), 1, true, 102).is_err());
    let cancelled = cancel(&s).unwrap();
    assert!(decide_request(&cancelled, &b, &h.owner_id(), 0, true, 102).is_err());
}
#[test]
fn join_saves_exact_owner_invite_and_consumption_together() {
    let h = OwnerKeypair::generate();
    let g = OwnerKeypair::generate();
    let mut s = Settings::default();
    let (b, p) = exchange::create_request(&g, "Friend", None, 0, 100).unwrap();
    s.exchange.add_pending(p, 100).unwrap();
    let req = exchange::verify_request(&b, &h.owner_id(), 101).unwrap();
    let b = exchange::seal_response(&h, &req, "opaque-invite", 102).unwrap();
    let response = s.exchange.verify(&g, &b, 103).unwrap();
    assert!(s.admitted_owners.is_empty());
    let decline = decide_response(&s, &response, false, 103).unwrap();
    assert!(decline.admitted_owners.is_empty());
    assert!(decline.exchange.verify(&g, &b, 104).is_err());
    let next = decide_response(&s, &response, true, 103).unwrap();
    let root = tempfile::tempdir().unwrap();
    next.save(root.path()).unwrap();
    let next = Settings::load(root.path()).unwrap();
    assert_eq!(next.admitted_owners, vec![h.owner_id()]);
    assert_eq!(
        next.connection,
        Connection::Private {
            invite: Some("opaque-invite".into())
        }
    );
    assert!(next.exchange.verify(&g, &b, 104).is_err());
    assert!(decide_response(&cancel(&s).unwrap(), &response, true, 104).is_err());
}
#[test]
fn public_host_cannot_approve_and_failed_save_does_not_change_current() {
    let h = OwnerKeypair::generate();
    let g = OwnerKeypair::generate();
    let (b, _) = exchange::create_request(&g, "Friend", None, 0, 100).unwrap();
    assert!(decide_request(&Settings::default(), &b, &h.owner_id(), 0, true, 101).is_err());
    let s = private_settings();
    let next = decide_request(&s, &b, &h.owner_id(), 0, true, 101).unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("launcher.json")).unwrap();
    assert!(next.save(root.path()).is_err());
    assert!(s.admitted_owners.is_empty());
    assert!(decide_request(&s, &b, &h.owner_id(), 0, true, 102).is_ok());
}

#[test]
fn later_approval_preserves_earlier_undelivered_reply() {
    let host = OwnerKeypair::generate();
    let a = OwnerKeypair::generate();
    let b = OwnerKeypair::generate();
    let settings = private_settings();
    let (ra, _) = exchange::create_request(&a, "Alice", None, 0, 100).unwrap();
    let (rb, _) = exchange::create_request(&b, "Bob", None, 0, 100).unwrap();
    let settings = decide_request(&settings, &ra, &host.owner_id(), 0, true, 101).unwrap();
    let settings = decide_request(&settings, &rb, &host.owner_id(), 0, true, 102).unwrap();
    assert_eq!(settings.replies.len(), 2);
    let root = tempfile::tempdir().unwrap();
    settings.save(root.path()).unwrap();
    let settings = Settings::load(root.path()).unwrap();
    assert_eq!(
        settings.replies[0]
            .verify(&settings, &host.owner_id(), 103)
            .unwrap()
            .owner_id(),
        a.owner_id()
    );
    assert_eq!(
        settings.replies[1]
            .verify(&settings, &host.owner_id(), 103)
            .unwrap()
            .owner_id(),
        b.owner_id()
    );
}

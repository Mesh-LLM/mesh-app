//! Live round trip against a running Mesh. Ignored by default; touches no funds.
//!
//! MESH_TRAY_LIVE_PORT=3232 MESH_TRAY_LIVE_PID=<tray pid> \
//!   cargo test --test live_wallet -- --ignored
//!
//! Saves a policy (free_only, so it can never spend) and a price for a dummy
//! model, reads both back, then restores the original policy and removes the
//! dummy price.
use mesh_tray::payments::{Client, Command, Mode, Policy, PolicyStatus, Pricing};
use std::collections::BTreeMap;

const DUMMY_MODEL: &str = "mesh-tray-live-check";

fn client() -> Client {
    let port = std::env::var("MESH_TRAY_LIVE_PORT").expect("MESH_TRAY_LIVE_PORT");
    let pid = std::env::var("MESH_TRAY_LIVE_PID").expect("MESH_TRAY_LIVE_PID");
    Client::new(port.parse().unwrap(), pid.parse().unwrap())
}

fn policy(c: &Client) -> PolicyStatus {
    c.execute(&Command::Policy { value: None })
        .expect("read policy")
}

fn pricing(c: &Client) -> BTreeMap<String, Pricing> {
    c.execute(&Command::Pricing).expect("read pricing")
}

#[test]
#[ignore]
fn saves_read_back_and_restore() {
    let c = client();
    let original = policy(&c);
    assert!(
        !pricing(&c).contains_key(DUMMY_MODEL),
        "leftover dummy price"
    );

    let test_policy = Policy {
        mode: Mode::FreeOnly,
        daily_budget_msat: Some(1_234_000),
    };
    let _: serde_json::Value = c
        .execute(&Command::Policy {
            value: Some(test_policy),
        })
        .expect("save policy");
    let saved = policy(&c);

    let price = Pricing {
        input_msat_per_million: 100_000,
        output_msat_per_million: 100_000,
        minimum_invoice_msat: 10_000,
    };
    let _: serde_json::Value = c
        .execute(&Command::SetPricing {
            model: DUMMY_MODEL.into(),
            value: Some(price.clone()),
        })
        .expect("save price");
    let priced = pricing(&c).get(DUMMY_MODEL).cloned();

    // Restore before asserting so a failure never leaves changes behind.
    let _: serde_json::Value = c
        .execute(&Command::SetPricing {
            model: DUMMY_MODEL.into(),
            value: None,
        })
        .expect("remove price");
    let _: serde_json::Value = c
        .execute(&Command::Policy {
            value: Some(Policy {
                mode: original.mode.clone(),
                daily_budget_msat: original.daily_budget_msat,
            }),
        })
        .expect("restore policy");

    assert_eq!(saved.mode, Mode::FreeOnly);
    assert_eq!(saved.daily_budget_msat, Some(1_234_000));
    assert_eq!(priced, Some(price));
    let restored = policy(&c);
    assert_eq!(restored.mode, original.mode);
    assert_eq!(restored.daily_budget_msat, original.daily_budget_msat);
    assert!(!pricing(&c).contains_key(DUMMY_MODEL));
}

//! Live round trip against a running Mesh. Ignored by default; touches no funds.
//!
//! MESH_TRAY_LIVE_PORT=3232 MESH_TRAY_LIVE_PID=<tray pid> \
//!   cargo test --test live_wallet -- --ignored
//!
//! Saves a policy (free_only, so it can never spend) and a price for a dummy
//! model and reads both back. A drop guard restores the original policy and
//! removes the dummy price on every exit path, including panics. Run it only
//! against a throwaway profile unless you accept that best-effort restore.
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

/// Puts the original policy back and removes the dummy price on every exit
/// path, including panics after the first write. Best effort: each step is
/// attempted even if another fails, and failures are reported, not hidden.
struct Restore<'a> {
    client: &'a Client,
    policy: Policy,
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        let price: Result<serde_json::Value, _> = self.client.execute(&Command::SetPricing {
            model: DUMMY_MODEL.into(),
            value: None,
        });
        let policy: Result<serde_json::Value, _> = self.client.execute(&Command::Policy {
            value: Some(self.policy.clone()),
        });
        if price.is_err() || policy.is_err() {
            eprintln!(
                "RESTORE FAILED (price removed: {}, policy restored: {}); original policy was {:?} and dummy model {DUMMY_MODEL}",
                price.is_ok(),
                policy.is_ok(),
                self.policy
            );
        }
    }
}

#[test]
#[ignore]
fn saves_read_back_and_restore() {
    let c = client();
    // Reads only until the guard exists; nothing has been changed yet.
    let original = policy(&c);
    assert!(
        !pricing(&c).contains_key(DUMMY_MODEL),
        "leftover dummy price"
    );
    let guard = Restore {
        client: &c,
        policy: Policy {
            mode: original.mode.clone(),
            daily_budget_msat: original.daily_budget_msat,
        },
    };

    let _: serde_json::Value = c
        .execute(&Command::Policy {
            value: Some(Policy {
                mode: Mode::FreeOnly,
                daily_budget_msat: Some(1_234_000),
            }),
        })
        .expect("save policy");
    let saved = policy(&c);
    assert_eq!(saved.mode, Mode::FreeOnly);
    assert_eq!(saved.daily_budget_msat, Some(1_234_000));

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
    assert_eq!(pricing(&c).get(DUMMY_MODEL), Some(&price));

    drop(guard);
    let restored = policy(&c);
    assert_eq!(restored.mode, original.mode);
    assert_eq!(restored.daily_budget_msat, original.daily_budget_msat);
    assert!(!pricing(&c).contains_key(DUMMY_MODEL));
}

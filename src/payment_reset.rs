//! Reset preferences through the engine API; never open or replace wallet files.
use crate::payments::{Client, Command, Mode, Policy, PolicyStatus, Pricing};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn disable(client: &Client) -> Result<(), String> {
    disable_with(|command| client.execute(command).map_err(|e| format!("{e:?}")))
}

fn disable_with(mut execute: impl FnMut(&Command) -> Result<Value, String>) -> Result<(), String> {
    execute(&Command::Policy {
        value: Some(Policy {
            mode: Mode::FreeOnly,
            daily_budget_msat: None,
        }),
    })?;
    let prices: BTreeMap<String, Pricing> =
        serde_json::from_value(execute(&Command::Pricing)?).map_err(|e| e.to_string())?;
    for model in prices.into_keys() {
        execute(&Command::SetPricing { model, value: None })?;
    }
    let policy: PolicyStatus = serde_json::from_value(execute(&Command::Policy { value: None })?)
        .map_err(|e| e.to_string())?;
    let prices: BTreeMap<String, Pricing> =
        serde_json::from_value(execute(&Command::Pricing)?).map_err(|e| e.to_string())?;
    if policy.mode != Mode::FreeOnly || policy.daily_budget_msat.is_some() || !prices.is_empty() {
        return Err(
            "Payment preferences did not remain disabled; another client may have changed them"
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy() -> Value {
        json!({"mode":"free_only", "daily_budget_msat":null, "spent_today_msat":12,
            "reserved_msat":3, "remaining_daily_budget_msat":0})
    }

    #[test]
    fn removes_every_price_and_verifies_without_wallet_or_history_commands() {
        let mut step = 0;
        disable_with(|command| {
            step += 1;
            match (step, command) {
                (1, Command::Policy { value: Some(p) }) => {
                    assert_eq!(p.mode, Mode::FreeOnly);
                    assert_eq!(p.daily_budget_msat, None);
                    Ok(policy())
                }
                (2, Command::Pricing) => Ok(json!({
                    "a":{"input_msat_per_million":1,"output_msat_per_million":1,"minimum_invoice_msat":1},
                    "b":{"input_msat_per_million":2,"output_msat_per_million":2,"minimum_invoice_msat":2}
                })),
                (3, Command::SetPricing { model, value: None }) if model == "a" => Ok(json!({})),
                (4, Command::SetPricing { model, value: None }) if model == "b" => Ok(json!({})),
                (5, Command::Policy { value: None }) => Ok(policy()),
                (6, Command::Pricing) => Ok(json!({})),
                _ => panic!("unexpected command"),
            }
        }).unwrap();
        assert_eq!(step, 6);
    }

    #[test]
    fn failure_stops_sequence_and_does_not_claim_rollback() {
        let mut calls = 0;
        let result = disable_with(|_| {
            calls += 1;
            if calls == 1 {
                Ok(policy())
            } else {
                Err("unreachable".into())
            }
        });
        assert_eq!(result, Err("unreachable".into()));
        assert_eq!(calls, 2);
    }

    #[test]
    fn detects_policy_changed_during_reset() {
        let mut calls = 0;
        let result = disable_with(|_| {
            calls += 1;
            Ok(match calls {
                1 => policy(),
                3 => {
                    let mut p = policy();
                    p["daily_budget_msat"] = json!(100);
                    p
                }
                _ => json!({}),
            })
        });
        assert!(result.is_err());
    }
}

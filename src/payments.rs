//! Local operator contract. Constructing this client performs no wallet calls.
//! Wallet reads/funding must follow an explicit user setup/action, never polling
//! on first mount. The engine ledger, not launcher preferences, owns policy.
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::time::Duration;

const MAX_RESPONSE: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    FreeOnly,
    Automatic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Policy {
    pub mode: Mode,
    pub daily_budget_msat: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pricing {
    pub input_msat_per_million: u64,
    pub output_msat_per_million: u64,
    pub minimum_invoice_msat: u64,
}

#[derive(Debug, Deserialize)]
pub struct PolicyStatus {
    pub mode: Mode,
    pub daily_budget_msat: Option<u64>,
    pub spent_today_msat: u64,
    pub reserved_msat: u64,
    pub remaining_daily_budget_msat: u64,
}

#[derive(Debug, Deserialize)]
pub struct Balance {
    pub spendable_msat: u64,
    pub available_for_inference_msat: u64,
}

// Deliberately not Debug: invoices should not leak through diagnostic dumps.
#[derive(Deserialize)]
pub struct FundingInvoice {
    pub bolt11: String,
    pub payment_hash: String,
    pub payee: String,
    pub amount_msat: Option<u64>,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Deserialize)]
pub struct Transaction {
    pub payment_hash: Option<String>,
    pub inbound: bool,
    pub amount_msat: u64,
    pub status: PaymentStatus,
}

/// What the wallet reports for one funding invoice. Absence from a bounded
/// list is `Unknown`, never "not paid".
#[derive(Debug, PartialEq, Eq)]
pub enum Receipt {
    Received(u64),
    Pending,
    Failed,
    Unknown,
}

pub fn receipt(transactions: &[Transaction], payment_hash: &str) -> Receipt {
    match transactions
        .iter()
        .find(|t| t.inbound && t.payment_hash.as_deref() == Some(payment_hash))
    {
        Some(t) => match t.status {
            PaymentStatus::Succeeded => Receipt::Received(t.amount_msat),
            PaymentStatus::Pending => Receipt::Pending,
            PaymentStatus::Failed => Receipt::Failed,
        },
        None => Receipt::Unknown,
    }
}

/// Side-effect free: Mesh keeps wallet state under `<profile>/payments`. A pin
/// or a legacy Lexe directory means a wallet already exists, so the menu shows
/// its balance instead of offering to create one. Never provisions anything.
pub fn wallet_exists(profile: &std::path::Path) -> bool {
    let payments = profile.join("payments");
    payments.join("wallet-provider.json").exists() || payments.join("lexe").is_dir()
}

/// `1234567` msat -> `"1,234 sats"`. Sub-sat remainders are truncated for
/// display only; every amount sent to Mesh stays exact msat.
pub fn format_sats(msat: u64) -> String {
    let digits = (msat / 1000).to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out} {}", if msat / 1000 == 1 { "sat" } else { "sats" })
}

#[derive(Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    Policy {
        value: Option<Policy>,
    },
    Pricing,
    SetPricing {
        model: String,
        value: Option<Pricing>,
    },
    Balance,
    /// `None` asks the payer's wallet to choose the amount.
    Fund {
        amount_msat: Option<u64>,
    },
    Transactions {
        limit: usize,
    },
}

impl Command {
    fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Policy {
                value: Some(policy),
            } if policy.mode == Mode::Automatic
                && !policy.daily_budget_msat.is_some_and(valid_amount) =>
            {
                Err(Error::InvalidInput("Enter a positive daily allowance"))
            }
            Self::SetPricing { model, value } => {
                if model.trim().is_empty() {
                    return Err(Error::InvalidInput("Select a served model"));
                }
                if value.as_ref().is_some_and(|price| {
                    !valid_amount(price.input_msat_per_million)
                        || !valid_amount(price.output_msat_per_million)
                        || !valid_amount(price.minimum_invoice_msat)
                }) {
                    return Err(Error::InvalidInput("Paid prices must be positive"));
                }
                Ok(())
            }
            Self::Fund {
                amount_msat: Some(amount_msat),
            } if !valid_amount(*amount_msat) => {
                Err(Error::InvalidInput("Enter a positive funding amount"))
            }
            _ => Ok(()),
        }
    }
}

fn valid_amount(value: u64) -> bool {
    value > 0 && value <= i64::MAX as u64
}

/// Exact whole-satoshi input for the first native forms. No floating point.
pub fn sats_to_msat(input: &str) -> Result<u64, Error> {
    let input = input.trim();
    if input.is_empty() || !input.bytes().all(|c| c.is_ascii_digit()) {
        return Err(Error::InvalidInput("Enter a whole number of sats"));
    }
    input
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(1000))
        .filter(|n| valid_amount(*n))
        .ok_or(Error::InvalidInput("Amount is zero or too large"))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidInput(&'static str),
    RuntimeChanged,
    Unavailable,
    Rejected(u16),
    Transport,
    InvalidResponse,
}

/// Only loopback, no redirects, bounded body, and no automatic write retries.
/// Call from a worker, never the native event loop. On RuntimeChanged the UI
/// must discard its snapshot and ask for confirmation again after refresh.
pub struct Client {
    port: u16,
    expected_pid: u32,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(port: u16, expected_pid: u32) -> Self {
        Self {
            port,
            expected_pid,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(10))
                .redirects(0)
                .build(),
        }
    }

    pub fn execute<T: serde::de::DeserializeOwned>(&self, command: &Command) -> Result<T, Error> {
        command.validate()?;
        let mut body =
            serde_json::to_value(command).map_err(|_| Error::InvalidInput("Invalid command"))?;
        body["expected_pid"] = self.expected_pid.into();
        let response = self
            .agent
            .post(&format!("http://127.0.0.1:{}/api/wallet", self.port))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|error| match error {
                ureq::Error::Status(409, _) => Error::RuntimeChanged,
                ureq::Error::Status(404 | 501, _) => Error::Unavailable,
                ureq::Error::Status(code, _) => Error::Rejected(code),
                ureq::Error::Transport(_) => Error::Transport,
            })?;
        if response.status() != 200 {
            return Err(Error::Rejected(response.status()));
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Transport)?;
        if bytes.len() as u64 > MAX_RESPONSE {
            return Err(Error::InvalidResponse);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse)
    }
}

#[cfg(test)]
mod tests;

//! Approximate US dollar value of sats, for display only. The rate comes from
//! free, keyless public price endpoints and is refreshed in the background;
//! nothing about payments depends on it.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// How often the BTC/USD rate is refreshed.
pub const EVERY: Duration = Duration::from_secs(10 * 60);

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .redirects(0)
        .build()
}

/// Coinbase: `{"data":{"amount":"67000.12",...}}`.
fn parse_coinbase(v: &serde_json::Value) -> Option<f64> {
    v["data"]["amount"].as_str()?.parse().ok()
}

/// CoinGecko: `{"bitcoin":{"usd":67000.12}}`.
fn parse_coingecko(v: &serde_json::Value) -> Option<f64> {
    v["bitcoin"]["usd"].as_f64()
}

fn sane(rate: f64) -> Option<f64> {
    (rate.is_finite() && rate > 1.0).then_some(rate)
}

/// One fresh BTC/USD read, trying each source in turn.
pub fn fetch_usd_per_btc() -> Result<f64, String> {
    type Parse = fn(&serde_json::Value) -> Option<f64>;
    let sources: [(&str, Parse); 2] = [
        (
            "https://api.coinbase.com/v2/prices/BTC-USD/spot",
            parse_coinbase,
        ),
        (
            "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd",
            parse_coingecko,
        ),
    ];
    let mut last = String::from("no price source");
    for (url, parse) in sources {
        match agent().get(url).call() {
            Ok(r) => match r.into_string().map_err(|e| e.to_string()).and_then(|t| {
                serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string())
            }) {
                Ok(v) => match parse(&v).and_then(sane) {
                    Some(rate) => return Ok(rate),
                    None => last = format!("{url}: unexpected reply"),
                },
                Err(e) => last = format!("{url}: {e}"),
            },
            Err(e) => last = format!("{url}: {e}"),
        }
    }
    Err(last)
}

/// Keep `cents` (USD cents per BTC, 0 = unknown) refreshed in the background.
pub fn spawn_refresher() -> Arc<AtomicU64> {
    let cents = Arc::new(AtomicU64::new(0));
    let out = cents.clone();
    std::thread::spawn(move || loop {
        if let Ok(rate) = fetch_usd_per_btc() {
            out.store((rate * 100.0).round() as u64, Ordering::Relaxed);
        }
        std::thread::sleep(EVERY);
    });
    cents
}

/// "≈ $1.23" for `sats` at `usd_per_btc`; small amounts keep precision.
pub fn usd(sats: u64, usd_per_btc: f64) -> String {
    let v = sats as f64 * usd_per_btc / 100_000_000.0;
    if sats == 0 {
        "$0".into()
    } else if v < 0.01 {
        "< $0.01".into()
    } else if v < 1000.0 {
        format!("${v:.2}")
    } else {
        let whole = (v.round() as u64).to_string();
        let mut out = String::from("$");
        for (i, c) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_both_sources_and_rejects_nonsense() {
        assert_eq!(
            parse_coinbase(&json!({"data":{"amount":"67000.50","base":"BTC"}})),
            Some(67000.5)
        );
        assert_eq!(
            parse_coingecko(&json!({"bitcoin":{"usd":67000.5}})),
            Some(67000.5)
        );
        assert_eq!(parse_coinbase(&json!({"data":{"amount":"abc"}})), None);
        assert_eq!(parse_coingecko(&json!({})), None);
        assert_eq!(sane(0.0), None);
        assert_eq!(sane(f64::NAN), None);
    }

    #[test]
    fn formats_dollars_for_small_and_large_amounts() {
        let rate = 100_000.0; // 1 sat = $0.001
        assert_eq!(usd(0, rate), "$0");
        assert_eq!(usd(5, rate), "< $0.01");
        assert_eq!(usd(1_000, rate), "$1.00");
        assert_eq!(usd(123_456, rate), "$123.46");
        assert_eq!(usd(2_000_000, rate), "$2,000");
    }
}

//! When to share compute: an optional sunshine condition that
//! pause the user's "Share compute" choice without changing it. Separate from
//! payments; the only effect is whether the engine serves.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Shortwave irradiance (W/m²) treated as "sunny enough". Roughly a bright,
/// partly cloudy day; full sun at noon is ~800–1000.
pub const SUNNY_W_M2: f64 = 200.0;
/// Weather is re-fetched at most this often.
pub const WEATHER_EVERY: Duration = Duration::from_secs(10 * 60);
/// AEMO dispatch runs every 5 minutes.
pub const GRID_EVERY: Duration = Duration::from_secs(5 * 60);
/// A changed answer must hold this many consecutive checks before it flips
/// sharing, so a passing cloud does not restart the engine.
pub const STABLE_CHECKS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ShareRules {
    pub sunny: bool,
    /// Share only while NSW wholesale power is free or negative (surplus).
    pub nsw_surplus: bool,
    /// Typed override; `None` means locate by IP address.
    pub location: Option<Location>,
}

/// What the conditions currently say. `None` means unknown (offline, or not
/// fetched yet); unknown never pauses on its own.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Readings {
    pub irradiance: Option<f64>,
    /// NSW1 dispatch price, $/MWh.
    pub nsw_price: Option<f64>,
}

/// `Ok(())` to share, or `Err(reason)` shown in the menu.
pub fn evaluate(rules: &ShareRules, now: Readings) -> Result<(), String> {
    if let Some(w) = now.irradiance.filter(|w| rules.sunny && *w < SUNNY_W_M2) {
        return Err(format!("Paused: not sunny ({w:.0} W/m²)"));
    }
    if let Some(p) = now.nsw_price.filter(|p| rules.nsw_surplus && *p > 0.0) {
        return Err(format!("Paused: NSW grid has no spare power (${p:.0}/MWh)"));
    }
    Ok(())
}

/// Debounces [`evaluate`]: the decision only changes after
/// [`STABLE_CHECKS`] consecutive agreeing checks.
#[derive(Debug, Clone)]
pub struct Gate {
    decided: Result<(), String>,
    candidate: Option<(bool, u32)>,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            decided: Ok(()),
            candidate: None,
        }
    }
}

impl Gate {
    /// First reading decides at once (no flap at launch).
    pub fn starting(result: Result<(), String>) -> Self {
        Self {
            decided: result,
            candidate: None,
        }
    }

    pub fn allows(&self) -> bool {
        self.decided.is_ok()
    }

    pub fn status(&self) -> &Result<(), String> {
        &self.decided
    }

    /// Feed one check; returns true when the share/pause decision flipped.
    pub fn observe(&mut self, result: Result<(), String>) -> bool {
        let allow = result.is_ok();
        if allow == self.allows() {
            self.candidate = None;
            // Same decision, maybe a fresher reason.
            self.decided = result;
            return false;
        }
        let seen = match self.candidate {
            Some((a, n)) if a == allow => n + 1,
            _ => 1,
        };
        if seen >= STABLE_CHECKS {
            self.decided = result;
            self.candidate = None;
            true
        } else {
            self.candidate = Some((allow, seen));
            false
        }
    }
}

/// Approximate location from this machine's public IP (city-level, via
/// ipwho.is, no key). Used only when the user has not typed one.
pub fn ip_location() -> Result<Location, String> {
    let body: serde_json::Value = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .get("https://ipwho.is/")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))?;
    parse_ip_location(&body)
}

pub fn parse_ip_location(body: &serde_json::Value) -> Result<Location, String> {
    match (body["latitude"].as_f64(), body["longitude"].as_f64()) {
        (Some(lat), Some(lon)) => Ok(Location { lat, lon }),
        _ => Err("IP lookup had no location".into()),
    }
}

/// NSW1 wholesale price from AEMO's public 5-minute NEM summary (no key).
/// Zero or negative means the grid has more power than it can use.
pub fn nsw_price() -> Result<f64, String> {
    let body: serde_json::Value = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .get("https://visualisations.aemo.com.au/aemo/apps/api/report/ELEC_NEM_SUMMARY")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))?;
    parse_nsw_price(&body)
}

pub fn parse_nsw_price(body: &serde_json::Value) -> Result<f64, String> {
    body["ELEC_NEM_SUMMARY"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["REGIONID"] == "NSW1"))
        .and_then(|r| r["PRICE"].as_f64())
        .ok_or_else(|| "AEMO summary had no NSW1 price".into())
}

/// Current shortwave irradiance from Open-Meteo (free, no key). The
/// location is the one the user typed; nothing is inferred from the network.
pub fn irradiance(at: Location) -> Result<f64, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.3}&longitude={:.3}&current=shortwave_radiation",
        at.lat, at.lon
    );
    let body: serde_json::Value = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))?;
    parse_irradiance(&body)
}

pub fn parse_irradiance(body: &serde_json::Value) -> Result<f64, String> {
    body["current"]["shortwave_radiation"]
        .as_f64()
        .ok_or_else(|| "weather reply had no shortwave_radiation".into())
}

/// "lat, lon" as typed, e.g. "-33.87, 151.21".
pub fn parse_location(text: &str) -> Result<Location, String> {
    let mut parts = text.split(',').map(str::trim);
    let (Some(a), Some(b), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err("Enter latitude, longitude — for example -33.87, 151.21".into());
    };
    let lat: f64 = a
        .parse()
        .map_err(|_| format!("Latitude '{a}' is not a number"))?;
    let lon: f64 = b
        .parse()
        .map_err(|_| format!("Longitude '{b}' is not a number"))?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err("Latitude must be −90…90 and longitude −180…180".into());
    }
    Ok(Location { lat, lon })
}

/// Background sampler: weather at most every [`WEATHER_EVERY`], or at once
/// when the location changes. The IP location is looked up once per run.
#[derive(Default)]
pub struct Sampler {
    ip: Option<(Instant, Option<Location>)>,
    weather: Option<(Location, Instant, Option<f64>)>,
    grid: Option<(Instant, Option<f64>)>,
}

impl Sampler {
    fn location(&mut self, rules: &ShareRules) -> Option<Location> {
        if rules.location.is_some() {
            return rules.location;
        }
        let stale = self
            .ip
            .is_none_or(|(t, l)| l.is_none() && t.elapsed() >= WEATHER_EVERY);
        if stale {
            self.ip = Some((Instant::now(), ip_location().ok()));
        }
        self.ip.and_then(|(_, l)| l)
    }

    pub fn sample(&mut self, rules: &ShareRules) -> Readings {
        Readings {
            irradiance: self.sunshine(rules),
            nsw_price: self.grid(rules),
        }
    }

    fn grid(&mut self, rules: &ShareRules) -> Option<f64> {
        if !rules.nsw_surplus {
            return None;
        }
        if self.grid.is_none_or(|(t, _)| t.elapsed() >= GRID_EVERY) {
            let previous = self.grid.and_then(|(_, p)| p);
            self.grid = Some((Instant::now(), nsw_price().ok().or(previous)));
        }
        self.grid.and_then(|(_, p)| p)
    }

    fn sunshine(&mut self, rules: &ShareRules) -> Option<f64> {
        if !rules.sunny {
            return None;
        }
        let at = self.location(rules)?;
        let fresh = self
            .weather
            .is_some_and(|(l, t, _)| l == at && t.elapsed() < WEATHER_EVERY);
        if !fresh {
            let previous = self
                .weather
                .and_then(|(l, _, w)| (l == at).then_some(w))
                .flatten();
            let w = irradiance(at).ok().or(previous);
            self.weather = Some((at, Instant::now(), w));
        }
        self.weather.and_then(|(_, _, w)| w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYD: Location = Location {
        lat: -33.87,
        lon: 151.21,
    };

    #[test]
    fn no_rules_always_share() {
        let dim = Readings {
            irradiance: Some(10.0),
            nsw_price: None,
        };
        assert!(evaluate(&ShareRules::default(), dim).is_ok());
    }

    #[test]
    fn sunny_pauses_only_on_a_dim_reading() {
        let rules = ShareRules {
            sunny: true,
            ..Default::default()
        };
        assert!(
            evaluate(&rules, Readings::default()).is_ok(),
            "unknown never pauses"
        );
        let dim = Readings {
            irradiance: Some(40.0),
            nsw_price: None,
        };
        assert!(evaluate(&rules, dim).unwrap_err().contains("not sunny"));
        let bright = Readings {
            irradiance: Some(650.0),
            nsw_price: None,
        };
        assert!(evaluate(&rules, bright).is_ok());
    }

    #[test]
    fn nsw_surplus_shares_only_at_zero_or_negative_price() {
        let rules = ShareRules {
            nsw_surplus: true,
            ..Default::default()
        };
        let at = |p| Readings {
            irradiance: None,
            nsw_price: Some(p),
        };
        assert!(evaluate(&rules, at(85.0)).unwrap_err().contains("NSW"));
        assert!(evaluate(&rules, at(0.0)).is_ok());
        assert!(evaluate(&rules, at(-40.0)).is_ok());
        assert!(evaluate(&rules, Readings::default()).is_ok());
        let body = serde_json::json!({"ELEC_NEM_SUMMARY": [
            {"REGIONID": "QLD1", "PRICE": 1.0},
            {"REGIONID": "NSW1", "PRICE": -12.5}
        ]});
        assert_eq!(parse_nsw_price(&body).unwrap(), -12.5);
    }

    #[test]
    fn gate_needs_consecutive_agreement() {
        let mut g = Gate::starting(Ok(()));
        assert!(!g.observe(Err("x".into())));
        assert!(g.allows());
        assert!(!g.observe(Ok(())), "a single agreeing check resets");
        assert!(!g.observe(Err("x".into())));
        assert!(g.observe(Err("x".into())));
        assert!(!g.allows());
    }

    #[test]
    fn parses_inputs() {
        assert_eq!(parse_location(" -33.87 , 151.21 ").unwrap(), SYD);
        assert!(parse_location("sydney").is_err());
        assert!(parse_location("100, 0").is_err());
        let ip = serde_json::json!({"latitude": -33.87, "longitude": 151.21});
        assert_eq!(parse_ip_location(&ip).unwrap(), SYD);
        assert!(parse_ip_location(&serde_json::json!({"success": false})).is_err());
        let body = serde_json::json!({"current": {"shortwave_radiation": 512.0}});
        assert_eq!(parse_irradiance(&body).unwrap(), 512.0);
    }
}

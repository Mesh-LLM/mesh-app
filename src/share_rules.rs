//! When to share compute: optional conditions on power and sunshine that
//! pause the user's "Share compute" choice without changing it. Separate from
//! payments; the only effect is whether the engine serves.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Shortwave irradiance (W/m²) treated as "sunny enough". Roughly a bright,
/// partly cloudy day; full sun at noon is ~800–1000.
pub const SUNNY_W_M2: f64 = 200.0;
/// Weather is re-fetched at most this often.
pub const WEATHER_EVERY: Duration = Duration::from_secs(10 * 60);
/// A changed answer must hold this many consecutive checks before it flips
/// sharing, so a passing cloud or a cable wiggle does not restart the engine.
pub const STABLE_CHECKS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ShareRules {
    pub plugged_in: bool,
    pub sunny: bool,
    pub location: Option<Location>,
}

impl ShareRules {
    pub fn any(&self) -> bool {
        self.plugged_in || self.sunny
    }
}

/// What the conditions currently say. `None` means unknown (e.g. weather
/// not fetched yet, or offline); unknown never pauses on its own.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Readings {
    pub on_mains: Option<bool>,
    pub irradiance: Option<f64>,
}

/// `Ok(())` to share, or `Err(reason)` shown in the menu.
pub fn evaluate(rules: &ShareRules, now: Readings) -> Result<(), String> {
    if rules.plugged_in && now.on_mains == Some(false) {
        return Err("Paused: on battery".into());
    }
    if rules.sunny {
        if rules.location.is_none() {
            return Err("Paused: set your location".into());
        }
        if let Some(w) = now.irradiance {
            if w < SUNNY_W_M2 {
                return Err(format!("Paused: not sunny ({w:.0} W/m²)"));
            }
        }
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

/// Whether the machine is on mains power. Desktops without a battery
/// report mains; unknown platforms report `None`.
pub fn on_mains() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/pmset")
            .args(["-g", "batt"])
            .output()
            .ok()?;
        parse_pmset(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(target_os = "linux")]
    {
        let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
        let mut saw_battery = false;
        for entry in dir.flatten() {
            let p = entry.path();
            let kind = std::fs::read_to_string(p.join("type")).unwrap_or_default();
            match kind.trim() {
                "Mains" => {
                    if std::fs::read_to_string(p.join("online")).is_ok_and(|s| s.trim() == "1") {
                        return Some(true);
                    }
                }
                "Battery" => saw_battery = true,
                _ => {}
            }
        }
        Some(!saw_battery)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

pub fn parse_pmset(text: &str) -> Option<bool> {
    if text.contains("'AC Power'") {
        Some(true)
    } else if text.contains("'Battery Power'") {
        Some(false)
    } else {
        None
    }
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

/// Background sampler: power every call, weather at most every
/// [`WEATHER_EVERY`] (or immediately when the location changes).
#[derive(Default)]
pub struct Sampler {
    weather: Option<(Location, Instant, Option<f64>)>,
}

impl Sampler {
    pub fn sample(&mut self, rules: &ShareRules) -> Readings {
        let on_mains = rules.plugged_in.then(on_mains).flatten();
        let irradiance = match (rules.sunny, rules.location) {
            (true, Some(at)) => {
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
            _ => None,
        };
        Readings {
            on_mains,
            irradiance,
        }
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
        assert!(evaluate(&ShareRules::default(), Readings::default()).is_ok());
    }

    #[test]
    fn battery_pauses_only_when_rule_on() {
        let r = Readings {
            on_mains: Some(false),
            irradiance: None,
        };
        assert!(evaluate(&ShareRules::default(), r).is_ok());
        let rules = ShareRules {
            plugged_in: true,
            ..Default::default()
        };
        assert_eq!(evaluate(&rules, r).unwrap_err(), "Paused: on battery");
        assert!(
            evaluate(&rules, Readings::default()).is_ok(),
            "unknown never pauses"
        );
    }

    #[test]
    fn sunny_needs_location_and_light() {
        let mut rules = ShareRules {
            sunny: true,
            ..Default::default()
        };
        assert!(evaluate(&rules, Readings::default()).is_err());
        rules.location = Some(SYD);
        let dim = Readings {
            on_mains: None,
            irradiance: Some(40.0),
        };
        assert!(evaluate(&rules, dim).unwrap_err().contains("not sunny"));
        let bright = Readings {
            irradiance: Some(650.0),
            ..dim
        };
        assert!(evaluate(&rules, bright).is_ok());
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
        assert_eq!(
            parse_pmset("Now drawing from 'AC Power'\n -InternalBattery-0"),
            Some(true)
        );
        assert_eq!(parse_pmset("Now drawing from 'Battery Power'"), Some(false));
        let body = serde_json::json!({"current": {"shortwave_radiation": 512.0}});
        assert_eq!(parse_irradiance(&body).unwrap(), 512.0);
    }
}

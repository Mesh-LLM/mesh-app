//! When to share compute: optional spare-power and time-of-day conditions that
//! pause the user's "Share compute" choice without changing it. Separate from
//! payments; the only effect is whether the engine serves.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Shortwave irradiance (W/m²) treated as "sunny enough". Roughly a bright,
/// partly cloudy day; full sun at noon is ~800–1000.
pub const SUNNY_W_M2: f64 = 200.0;
/// Weather is re-fetched at most this often.
pub const WEATHER_EVERY: Duration = Duration::from_secs(10 * 60);
/// A changed answer must hold this many consecutive checks (15s apart) before
/// it flips sharing. Weather is cached for 10 minutes, so these are usually
/// re-evaluations of the same reading: this guards against a single odd
/// evaluation (e.g. a clock edge), not a passing cloud.
pub const STABLE_CHECKS: u32 = 2;
/// A reading older than this is treated as unknown rather than reused.
pub const STALE_AFTER: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ShareRules {
    /// Share only when sunny here. Keep the persisted name for compatibility.
    pub spare_power: bool,
    /// Share only between `hours` (local time).
    pub hours_on: bool,
    pub hours: Hours,
    /// Typed override; `None` means locate by IP address.
    pub location: Option<Location>,
}

impl Default for ShareRules {
    fn default() -> Self {
        Self {
            spare_power: false,
            hours_on: false,
            hours: Hours {
                start: 10 * 60,
                end: 15 * 60,
            },
            location: None,
        }
    }
}

impl ShareRules {
    pub fn any(&self) -> bool {
        self.spare_power || self.hours_on
    }
}

/// A daily window in minutes since local midnight; `end < start` wraps
/// past midnight (e.g. 22:00–06:00).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Hours {
    pub start: u16,
    pub end: u16,
}

impl Hours {
    pub fn contains(self, minute: u16) -> bool {
        if self.start <= self.end {
            (self.start..self.end).contains(&minute)
        } else {
            minute >= self.start || minute < self.end
        }
    }

    pub fn label(self) -> String {
        let f = |m: u16| format!("{:02}:{:02}", m / 60, m % 60);
        format!("{}–{}", f(self.start), f(self.end))
    }
}

/// "10:00-15:00" (also accepts "10-15" and an en dash).
pub fn parse_hours(text: &str) -> Result<Hours, String> {
    let bad = || "Enter hours like 10:00-15:00".to_string();
    let one = |t: &str| -> Result<u16, String> {
        let t = t.trim();
        let (h, m) = t.split_once(':').unwrap_or((t, "0"));
        let h: u16 = h.trim().parse().map_err(|_| bad())?;
        let m: u16 = m.trim().parse().map_err(|_| bad())?;
        (h < 24 && m < 60).then_some(h * 60 + m).ok_or_else(bad)
    };
    let (a, b) = text
        .split_once('-')
        .or_else(|| text.split_once('–'))
        .ok_or_else(bad)?;
    let hours = Hours {
        start: one(a)?,
        end: one(b)?,
    };
    (hours.start != hours.end).then_some(hours).ok_or_else(bad)
}

/// What the conditions currently say. `None` means unknown (offline, or not
/// fetched yet).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Readings {
    pub irradiance: Option<f64>,
    /// Local minutes since midnight.
    pub minute: Option<u16>,
}

impl Readings {
    /// Clock only: no network, so it can decide at launch without blocking.
    pub fn clock() -> Self {
        Self {
            minute: Some(local_minute()),
            ..Default::default()
        }
    }

    /// Sunshine condition is on but weather has no usable reading.
    pub fn power_unknown(&self, rules: &ShareRules) -> bool {
        rules.spare_power && self.irradiance.is_none()
    }
}

/// `Ok(())` to share, or `Err(reason)` shown in the menu.
pub fn evaluate(rules: &ShareRules, now: Readings) -> Result<(), String> {
    if rules.hours_on && now.minute.is_some_and(|m| !rules.hours.contains(m)) {
        return Err(format!("Paused: outside {}", rules.hours.label()));
    }
    if rules.spare_power {
        // Preserve the existing offline policy: unknown weather never pauses.
        if let Some(w) = now.irradiance.filter(|w| *w < SUNNY_W_M2) {
            return Err(format!("Paused: not sunny ({w:.0} W/m²)"));
        }
    }
    Ok(())
}

pub fn local_minute() -> u16 {
    use chrono::Timelike;
    let now = chrono::Local::now();
    (now.hour() * 60 + now.minute()) as u16
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
/// A good reading and when it was taken.
type Good = (f64, Instant);

#[derive(Default)]
pub struct Sampler {
    ip: Option<(Instant, Option<Location>)>,
    /// (location, last attempt, last good value and when it was read).
    weather: Option<(Location, Instant, Option<Good>)>,
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
        let at = rules.spare_power.then(|| self.location(rules)).flatten();
        Readings {
            irradiance: at.and_then(|at| self.sunshine(at)),
            minute: Some(local_minute()),
        }
    }

    fn sunshine(&mut self, at: Location) -> Option<f64> {
        let fresh = self
            .weather
            .is_some_and(|(l, t, _)| l == at && t.elapsed() < WEATHER_EVERY);
        if !fresh {
            let previous = self
                .weather
                .and_then(|(l, _, w)| (l == at).then_some(w))
                .flatten();
            let now = Instant::now();
            let w = irradiance(at).ok().map(|w| (w, now)).or(previous);
            self.weather = Some((at, now, w));
        }
        unless_stale(self.weather.and_then(|(_, _, w)| w))
    }
}

/// A failed refresh keeps the last good value only until it is stale.
fn unless_stale(reading: Option<Good>) -> Option<f64> {
    reading
        .filter(|(_, at)| at.elapsed() < STALE_AFTER)
        .map(|(v, _)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYD: Location = Location {
        lat: -33.87,
        lon: 151.21,
    };

    fn read(w: Option<f64>, minute: u16) -> Readings {
        Readings {
            irradiance: w,
            minute: Some(minute),
        }
    }

    #[test]
    fn launch_decides_from_clock_without_network() {
        let rules = ShareRules {
            spare_power: true,
            ..Default::default()
        };
        let now = Readings::clock();
        assert!(now.power_unknown(&rules));
        assert_eq!(evaluate(&rules, now), Ok(()));
    }

    #[test]
    fn stale_readings_become_unknown() {
        let now = Instant::now();
        assert_eq!(unless_stale(Some((5.0, now))), Some(5.0));
        let old = now.checked_sub(STALE_AFTER + Duration::from_secs(1));
        if let Some(old) = old {
            assert_eq!(unless_stale(Some((5.0, old))), None);
        }
        assert_eq!(unless_stale(None), None);
    }

    #[test]
    fn no_rules_always_share() {
        assert!(evaluate(&ShareRules::default(), read(Some(1.0), 0)).is_ok());
    }

    #[test]
    fn sunshine_threshold_and_unknown_weather() {
        let rules = ShareRules {
            spare_power: true,
            ..Default::default()
        };
        assert!(evaluate(&rules, read(Some(SUNNY_W_M2), 0)).is_ok());
        assert!(evaluate(&rules, read(Some(650.0), 0)).is_ok());
        assert_eq!(
            evaluate(&rules, read(Some(20.0), 0)).unwrap_err(),
            "Paused: not sunny (20 W/m²)"
        );
        assert!(
            evaluate(&rules, read(None, 0)).is_ok(),
            "unknown never pauses"
        );
        assert!(read(None, 0).power_unknown(&rules));
        assert!(!read(Some(0.0), 0).power_unknown(&rules));
    }

    #[test]
    fn existing_sunshine_preference_survives() {
        let rules: ShareRules = serde_json::from_str(r#"{"spare_power":true}"#).unwrap();
        assert!(rules.spare_power);
        assert!(evaluate(&rules, read(Some(0.0), 0)).is_err());
    }

    #[test]
    fn hours_window_including_overnight() {
        let mut rules = ShareRules {
            hours_on: true,
            ..Default::default()
        };
        assert!(evaluate(&rules, read(None, 11 * 60)).is_ok());
        assert_eq!(
            evaluate(&rules, read(None, 16 * 60)).unwrap_err(),
            "Paused: outside 10:00–15:00"
        );
        rules.hours = parse_hours("22:00-6").unwrap();
        assert!(rules.hours.contains(23 * 60) && rules.hours.contains(60));
        assert!(!rules.hours.contains(12 * 60));
        assert!(parse_hours("9").is_err() && parse_hours("10-10").is_err());
        assert!(parse_hours("25:00-3:00").is_err());
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

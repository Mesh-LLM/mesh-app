//! Share-when control: spare-power and time-of-day conditions that pause
//! sharing without changing the user's Share compute choice.
use mesh_tray::share_rules::{self, Gate, Readings, Sampler, ShareRules};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

/// How often the (cached) weather is re-evaluated.
const CHECK_EVERY: Duration = Duration::from_secs(15);

pub struct ShareControl {
    gate: Gate,
    rules_tx: Sender<ShareRules>,
    results: Receiver<(Result<(), String>, bool)>,
    /// No full reading yet: the next one decides without debounce.
    first: bool,
    /// Spare power is on but no fresh weather or grid reading exists.
    unknown: bool,
}

impl ShareControl {
    pub fn new(rules: &ShareRules) -> Self {
        let (rules_tx, rules_rx) = mpsc::channel::<ShareRules>();
        let (tx, results) = mpsc::channel();
        let first = rules.clone();
        // Launch decides from the clock alone (no network wait before the
        // engine starts); the first full reading, taken at once in the
        // background, then decides without debounce.
        let mut sampler = Sampler::default();
        let gate = Gate::starting(share_rules::evaluate(&first, Readings::clock()));
        std::thread::spawn(move || {
            let mut rules = first;
            loop {
                let now = sampler.sample(&rules);
                let unknown = now.power_unknown(&rules);
                if tx
                    .send((share_rules::evaluate(&rules, now), unknown))
                    .is_err()
                {
                    return;
                }
                match rules_rx.recv_timeout(CHECK_EVERY) {
                    Ok(next) => rules = next,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
                while let Ok(next) = rules_rx.try_recv() {
                    rules = next;
                }
            }
        });
        Self {
            gate,
            rules_tx,
            results,
            first: true,
            unknown: false,
        }
    }

    pub fn allows(&self) -> bool {
        self.gate.allows()
    }

    /// Handle a `share:` click; returns the new rules to save, if any.
    pub fn click(&mut self, id: &str, current: &ShareRules) -> Option<ShareRules> {
        let mut next = current.clone();
        match id {
            "share:spare" => next.spare_power = !next.spare_power,
            // Turning hours on asks for the window (prefilled); off just stops.
            "share:hours" if next.hours_on => next.hours_on = false,
            "share:hours" => {
                next.hours = ask_hours(next.hours)?;
                next.hours_on = true;
            }
            "share:location" => next.location = Some(ask_location()?),
            _ => return None,
        }
        let _ = self.rules_tx.send(next.clone());
        Some(next)
    }

    /// Drain background checks; returns true when share/pause flipped.
    pub fn tick(&mut self) -> bool {
        let mut flipped = false;
        while let Ok((result, unknown)) = self.results.try_recv() {
            self.unknown = unknown;
            if std::mem::take(&mut self.first) {
                let was = self.gate.allows();
                self.gate = Gate::starting(result);
                flipped |= was != self.gate.allows();
            } else {
                flipped |= self.gate.observe(result);
            }
        }
        flipped
    }

    /// Rules changed by a click (or a failed save): re-sync checks and sampler.
    pub fn reset(&mut self, rules: &ShareRules) {
        let _ = self.rules_tx.send(rules.clone());
    }

    /// One line for the panel: sharing now, or why it is paused.
    pub fn status_text(&self, rules: &ShareRules, user_sharing: bool) -> String {
        match (user_sharing, self.gate.status()) {
            (false, _) => "Sharing is off".to_string(),
            (true, Ok(())) if rules.spare_power && (self.first || self.unknown) => {
                "Sharing: power status unknown (checking weather and grid)".to_string()
            }
            (true, Ok(())) if rules.any() => "Sharing now".to_string(),
            (true, Ok(())) => "Sharing whenever Mesh runs".to_string(),
            (true, Err(reason)) => reason.clone(),
        }
    }
}

fn ask_hours(current: share_rules::Hours) -> Option<share_rules::Hours> {
    let text = crate::native::prompt_text(
        "Share between",
        "Local time, for example 10:00-15:00. Overnight works too: 22:00-06:00.",
        "Save",
        &current.label().replace('–', "-"),
    )?;
    match share_rules::parse_hours(&text) {
        Ok(h) => Some(h),
        Err(e) => {
            crate::native::notice("Hours not saved", &e);
            None
        }
    }
}

fn ask_location() -> Option<share_rules::Location> {
    let text = crate::native::prompt_text(
        "Where is your solar?",
        "By default Mesh uses your IP address's city. To be exact, enter latitude, longitude (for example -33.87, 151.21). Sunshine there is checked with Open-Meteo every 10 minutes.",
        "Save",
        "",
    )?;
    match share_rules::parse_location(&text) {
        Ok(l) => Some(l),
        Err(e) => {
            crate::native::notice("Location not saved", &e);
            None
        }
    }
}

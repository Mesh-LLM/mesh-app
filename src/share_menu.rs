//! "Share when" submenu: spare-power and time-of-day conditions that pause
//! sharing without changing the user's Share compute choice.
use mesh_tray::share_rules::{self, Gate, Sampler, ShareRules};
use muda::{CheckMenuItem, MenuItem, Submenu};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

/// How often the (cached) weather is re-evaluated.
const CHECK_EVERY: Duration = Duration::from_secs(15);

pub struct ShareControl {
    gate: Gate,
    rules_tx: Sender<ShareRules>,
    results: Receiver<Result<(), String>>,
    menu: Option<Items>,
}

struct Items {
    submenu: Submenu,
    spare: CheckMenuItem,
    hours: CheckMenuItem,
    location: MenuItem,
    status: MenuItem,
}

impl ShareControl {
    pub fn new(rules: &ShareRules) -> Self {
        let (rules_tx, rules_rx) = mpsc::channel::<ShareRules>();
        let (tx, results) = mpsc::channel();
        let first = rules.clone();
        // First decision is synchronous so launch never flaps.
        let mut sampler = Sampler::default();
        let gate = Gate::starting(share_rules::evaluate(&first, sampler.sample(&first)));
        std::thread::spawn(move || {
            let mut rules = first;
            loop {
                match rules_rx.recv_timeout(CHECK_EVERY) {
                    Ok(next) => rules = next,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
                while let Ok(next) = rules_rx.try_recv() {
                    rules = next;
                }
                let now = sampler.sample(&rules);
                if tx.send(share_rules::evaluate(&rules, now)).is_err() {
                    return;
                }
            }
        });
        Self {
            gate,
            rules_tx,
            results,
            menu: None,
        }
    }

    pub fn allows(&self) -> bool {
        self.gate.allows()
    }

    pub fn submenu(&mut self, rules: &ShareRules) -> Result<&Submenu, String> {
        let items = Items {
            submenu: Submenu::new("Share when", true),
            spare: CheckMenuItem::with_id(
                "share:spare",
                "Only when sunny or there's surplus energy",
                true,
                rules.spare_power,
                None,
            ),
            hours: CheckMenuItem::with_id(
                "share:hours",
                hours_label(rules),
                true,
                rules.hours_on,
                None,
            ),
            location: MenuItem::with_id("share:location", location_label(rules), true, None),
            status: MenuItem::with_id("share:status", "", false, None),
        };
        items
            .submenu
            .append_items(&[
                &items.status,
                &muda::PredefinedMenuItem::separator(),
                &items.spare,
                &muda::PredefinedMenuItem::separator(),
                &items.location,
            ])
            .map_err(|e| e.to_string())?;
        self.menu = Some(items);
        self.render(rules, true);
        Ok(&self.menu.as_ref().expect("just set").submenu)
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
    pub fn tick(&mut self, rules: &ShareRules, user_sharing: bool, render: bool) -> bool {
        let mut flipped = false;
        while let Ok(result) = self.results.try_recv() {
            flipped |= self.gate.observe(result);
        }
        if render {
            self.render(rules, user_sharing);
        }
        flipped
    }

    /// Rules changed by a click (or a failed save): re-sync checks and sampler.
    pub fn reset(&mut self, rules: &ShareRules) {
        let _ = self.rules_tx.send(rules.clone());
        if let Some(m) = &self.menu {
            m.spare.set_checked(rules.spare_power);
            m.hours.set_checked(rules.hours_on);
            m.hours.set_text(hours_label(rules));
            m.location.set_text(location_label(rules));
        }
    }

    /// One line for the panel: sharing now, or why it is paused.
    pub fn status_text(&self, rules: &ShareRules, user_sharing: bool) -> String {
        match (user_sharing, self.gate.status()) {
            (false, _) => "Sharing is off".to_string(),
            (true, Ok(())) if rules.any() => "Sharing now".to_string(),
            (true, Ok(())) => "Sharing whenever Mesh runs".to_string(),
            (true, Err(reason)) => reason.clone(),
        }
    }

    fn render(&self, rules: &ShareRules, user_sharing: bool) {
        let Some(m) = &self.menu else { return };
        let text = match (user_sharing, self.gate.status()) {
            (false, _) => "Sharing is off".to_string(),
            (true, Ok(())) if rules.any() => "Sharing now".to_string(),
            (true, Ok(())) => "Sharing whenever Mesh runs".to_string(),
            (true, Err(reason)) => reason.clone(),
        };
        if m.status.text() != text {
            m.status.set_text(text);
        }
    }
}

fn hours_label(rules: &ShareRules) -> String {
    format!("Between {}…", rules.hours.label())
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

fn location_label(rules: &ShareRules) -> String {
    match rules.location {
        Some(l) => format!("Location: {:.2}, {:.2}…", l.lat, l.lon),
        None => "Location: from IP address (change…)".into(),
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

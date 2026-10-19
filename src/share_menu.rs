//! "Share only when sunny" submenu: a sunshine condition that pauses
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
    sunny: CheckMenuItem,
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
            submenu: Submenu::new("Solar", true),
            sunny: CheckMenuItem::with_id(
                "share:sunny",
                "Only when sunny",
                true,
                rules.sunny,
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
                &items.sunny,
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
            "share:sunny" => next.sunny = !next.sunny,
            "share:location" => next.location = Some(ask_location()?),
            _ => return None,
        }
        let _ = self.rules_tx.send(next.clone());
        Some(next)
    }

    /// Drain background checks; returns true when share/pause flipped.
    pub fn tick(&mut self, rules: &ShareRules, user_sharing: bool) -> bool {
        let mut flipped = false;
        while let Ok(result) = self.results.try_recv() {
            flipped |= self.gate.observe(result);
        }
        self.render(rules, user_sharing);
        flipped
    }

    /// Rules changed by a click (or a failed save): re-sync checks and sampler.
    pub fn reset(&mut self, rules: &ShareRules) {
        let _ = self.rules_tx.send(rules.clone());
        if let Some(m) = &self.menu {
            m.sunny.set_checked(rules.sunny);
            m.location.set_text(location_label(rules));
        }
    }

    fn render(&self, rules: &ShareRules, user_sharing: bool) {
        let Some(m) = &self.menu else { return };
        let text = match (user_sharing, self.gate.status()) {
            (false, _) => "Sharing is off".to_string(),
            (true, Ok(())) if rules.sunny => "Sharing now".to_string(),
            (true, Ok(())) => "Sharing whenever Mesh runs".to_string(),
            (true, Err(reason)) => reason.clone(),
        };
        if m.status.text() != text {
            m.status.set_text(text);
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
    let text = crate::native::prompt_card(
        "Where is your solar?",
        "By default Mesh uses your IP address's city. To be exact, enter latitude, longitude (for example -33.87, 151.21). Sunshine there is checked with Open-Meteo every 10 minutes.",
        "Save",
    )?;
    match share_rules::parse_location(&text) {
        Ok(l) => Some(l),
        Err(e) => {
            crate::native::notice("Location not saved", &e);
            None
        }
    }
}

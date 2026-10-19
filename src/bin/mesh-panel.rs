//! Spike: a small read-only GPUI status panel, launched from the tray.
//! Reads the launcher's TOML settings and the engine's loopback status; it
//! never starts, stops or reconfigures Mesh.
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use mesh_tray::{model_selection, settings, settings::Connection, status};
use mesh_tray::payments::{format_sats, Client, Command, Pricing};
use std::collections::BTreeMap;
use std::time::Duration;

/// Price of the served model, read fresh from the engine (it owns prices).
fn price(port: u16, snap: &status::Snapshot) -> String {
    let (Some(pid), Some(model)) = (snap.pid, snap.serving_models.first()) else {
        return "— (no model loaded)".into();
    };
    match Client::new(port, pid).execute::<BTreeMap<String, Pricing>>(&Command::Pricing) {
        Ok(m) => match m.get(model) {
            Some(p) => format!(
                "{} / {} sats per 1M in/out",
                format_sats(p.input_msat_per_million),
                format_sats(p.output_msat_per_million)
            ),
            None => "free".into(),
        },
        Err(_) => "unknown".into(),
    }
}

struct Panel {
    settings: Option<settings::Settings>,
    snap: status::Snapshot,
    price: String,
    error: Option<String>,
}

impl Panel {
    fn refresh(&mut self) {
        match settings::data_root().and_then(|root| settings::Settings::load(&root)) {
            Ok(s) => {
                self.snap = status::snapshot(s.console_port);
                self.price = price(s.console_port, &self.snap);
                self.settings = Some(s);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
}

fn row(label: &'static str, value: String) -> Div {
    div()
        .flex()
        .justify_between()
        .gap_4()
        .child(div().text_color(rgb(0x8a8f98)).child(label))
        .child(div().child(value))
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let s = self.settings.clone().unwrap_or_default();
        let mode = match &s.connection {
            Connection::Automatic => "Public".to_string(),
            Connection::Private { invite: Some(_) } => "Private (joined)".to_string(),
            Connection::Private { invite: None } => "Private (own mesh)".to_string(),
        };
        let engine = if self.snap.running {
            status::describe(&self.snap)
        } else {
            "Not running".to_string()
        };
        let rules = &s.share_rules;
        let mut when = Vec::new();
        if rules.spare_power {
            when.push("spare power".to_string());
        }
        if rules.hours_on {
            when.push(rules.hours.label());
        }
        let when = if when.is_empty() { "always".into() } else { when.join(", ") };

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(rgb(0x14161a))
            .text_color(rgb(0xe6e8eb))
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(img(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/mesh-jellyfish.png")).size_10())
                    .child(div().text_lg().child("Mesh")),
            )
            .child(row("Status", engine))
            .child(row("Network", mode))
            .child(row("Share compute", if s.share_compute { "on".into() } else { "off".into() }))
            .child(row("Share when", when))
            .when(!self.snap.serving_models.is_empty(), |d| {
                d.child(row("Serving", self.snap.serving_models.join(", ")))
            })
            .child(row(
                "Model to share",
                model_selection::local_model(&s.connection)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "none fits this machine".into()),
            ))
            .child(row("Price", self.price.clone()))
            .when_some(self.error.clone(), |d, e| {
                d.child(div().text_color(rgb(0xff6b6b)).child(e))
            })
    }
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        let bounds = Bounds::centered(None, size(px(360.), px(260.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            kind: WindowKind::PopUp,
            ..Default::default()
        };
        let (_, view) = gpui_kit::open_window(options, cx, |_, cx| {
            cx.new(|_| {
                let mut p = Panel { settings: None, snap: Default::default(), price: String::new(), error: None };
                p.refresh();
                p
            })
        })
        .expect("open panel window");
        let view = view.downgrade();
        // Poll off the UI thread; the status call is blocking with a 2s timeout.
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let (s, snap, pr) = cx
                .background_executor()
                .spawn(async {
                    let s = settings::data_root().and_then(|r| settings::Settings::load(&r));
                    let snap = s.as_ref().map(|s| status::snapshot(s.console_port)).unwrap_or_default();
                    let pr = s.as_ref().map(|s| price(s.console_port, &snap)).unwrap_or_default();
                    (s, snap, pr)
                })
                .await;
            if view
                .update(cx, |p, cx| {
                    match s {
                        Ok(s) => {
                            p.settings = Some(s);
                            p.error = None;
                        }
                        Err(e) => p.error = Some(e),
                    }
                    p.snap = snap;
                    p.price = pr;
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}

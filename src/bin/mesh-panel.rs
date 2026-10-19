//! Mesh panel, opened from the tray. Shows engine status and edits the app's
//! own preferences in `~/.mesh-app/mesh-app.toml`: which ladder model to
//! share, its price, and "share now". The tray owns the engine and applies
//! these (restart for a model change, price once the model is served).
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use mesh_tray::app_config::{self, AppConfig, LADDER};
use mesh_tray::payments::{format_sats, Client, Command, Pricing};
use mesh_tray::{model_selection, settings, settings::Connection, status};
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
    app: AppConfig,
    error: Option<String>,
}

impl Panel {
    fn refresh(&mut self) {
        if let Ok(root) = settings::data_root() {
            self.app = AppConfig::load(&root);
        }
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

impl Panel {
    fn edit(&mut self, f: impl FnOnce(&mut AppConfig), cx: &mut Context<Self>) {
        f(&mut self.app);
        match settings::data_root().and_then(|root| self.app.save(&root)) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("Could not save: {e}")),
        }
        cx.notify();
    }

    /// The model the panel is editing: explicit choice, else the auto pick.
    fn current_model(&self, auto: Option<&str>) -> Option<String> {
        self.app.model.clone().or(auto.map(str::to_string))
    }
}

fn button(id: impl Into<ElementId>, text: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    div()
        .id(id.into())
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .bg(if on { rgb(0x2f6fed) } else { rgb(0x262a31) })
        .hover(|d| d.bg(rgb(0x3a404a)))
        .child(text.into())
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let when = if when.is_empty() {
            "always".into()
        } else {
            when.join(", ")
        };

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
                    .child(
                        img(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/assets/mesh-jellyfish.png"
                        ))
                        .size_10(),
                    )
                    .child(div().text_lg().child("Mesh")),
            )
            .child(row("Status", engine))
            .child(row("Network", mode))
            .child(row(
                "Share compute",
                if s.share_compute {
                    "on".into()
                } else {
                    "off".into()
                },
            ))
            .child(row("Share when", when))
            .when(!self.snap.serving_models.is_empty(), |d| {
                d.child(row("Serving", self.snap.serving_models.join(", ")))
            })
            .child(
                div()
                    .mt_2()
                    .text_color(rgb(0x8a8f98))
                    .child("Model to share"),
            )
            .child({
                let auto = model_selection::local_model(&s.connection).ok().flatten();
                let mut list = div().flex().flex_col().gap_1().child(
                    button(
                        "auto",
                        format!(
                            "Automatic ({})",
                            auto.as_deref()
                                .map(app_config::label)
                                .unwrap_or("none fits")
                        ),
                        self.app.model.is_none(),
                    )
                    .on_click(cx.listener(|p, _, _, cx| p.edit(|a| a.model = None, cx))),
                );
                for (i, rung) in LADDER.iter().enumerate() {
                    let fits = model_selection::fits(rung.model);
                    let on = self.app.model.as_deref() == Some(rung.model);
                    let text = format!(
                        "{}{}",
                        rung.label,
                        if fits {
                            ""
                        } else {
                            " — too big for this machine"
                        }
                    );
                    let b = button(("rung", i), text, on);
                    list = list.child(if fits {
                        b.on_click(cx.listener(move |p, _, _, cx| {
                            p.edit(|a| a.model = Some(LADDER[i].model.to_string()), cx)
                        }))
                    } else {
                        b.text_color(rgb(0x5c616b))
                    });
                }
                list
            })
            .child({
                let auto = model_selection::local_model(&s.connection).ok().flatten();
                let model = self.current_model(auto.as_deref());
                let mut price_row = div().flex().items_center().gap_2().mt_2();
                match model {
                    None => price_row = price_row.child("Price: no model to price"),
                    Some(model) => {
                        let saved = self.app.price_for(&model);
                        let suggested = LADDER
                            .iter()
                            .find(|r| r.model == model)
                            .map(|r| r.suggested);
                        let text = match saved {
                            Some(p) => {
                                format!("{} / {} sats per 1M in/out", p.input_sats, p.output_sats)
                            }
                            None => "Free".into(),
                        };
                        price_row = price_row
                            .child(div().text_color(rgb(0x8a8f98)).child("Price"))
                            .child(div().flex_1().child(text));
                        if let Some(sug) = suggested {
                            let m = model.clone();
                            price_row = price_row.child(
                                button(
                                    "suggested",
                                    format!("Suggested {}/{}", sug.input_sats, sug.output_sats),
                                    saved == Some(sug),
                                )
                                .on_click(cx.listener(
                                    move |p, _, _, cx| {
                                        let m = m.clone();
                                        p.edit(
                                            |a| {
                                                a.prices.insert(m, sug);
                                            },
                                            cx,
                                        )
                                    },
                                )),
                            );
                        }
                        for (id, f) in [("down", 0.5f64), ("up", 2.0)] {
                            let m = model.clone();
                            let base = saved.or(suggested);
                            price_row = price_row.child(
                                button(id, if id == "up" { "×2" } else { "½" }, false).on_click(
                                    cx.listener(move |p, _, _, cx| {
                                        let Some(b) = base else { return };
                                        let scale = |v: u64| ((v as f64 * f).round() as u64).max(1);
                                        let np = app_config::Price {
                                            input_sats: scale(b.input_sats),
                                            output_sats: scale(b.output_sats),
                                        };
                                        let m = m.clone();
                                        p.edit(
                                            |a| {
                                                a.prices.insert(m, np);
                                            },
                                            cx,
                                        )
                                    }),
                                ),
                            );
                        }
                        let m = model.clone();
                        price_row =
                            price_row.child(button("free", "Free", saved.is_none()).on_click(
                                cx.listener(move |p, _, _, cx| {
                                    let m = m.clone();
                                    p.edit(
                                        |a| {
                                            a.prices.insert(m, app_config::FREE);
                                        },
                                        cx,
                                    )
                                }),
                            ));
                    }
                }
                price_row
            })
            .child(row("Price now (engine)", self.price.clone()))
            .child(
                div().flex().items_center().gap_2().mt_2().child(
                    button(
                        "share-now",
                        if self.app.share_now {
                            "Sharing now (ignores Share when)"
                        } else {
                            "Share now, ignore Share when"
                        },
                        self.app.share_now,
                    )
                    .on_click(
                        cx.listener(|p, _, _, cx| p.edit(|a| a.share_now = !a.share_now, cx)),
                    ),
                ),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(div().text_color(rgb(0xff6b6b)).child(e))
            })
    }
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        let bounds = Bounds::centered(None, size(px(460.), px(520.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            kind: WindowKind::Normal,
            ..Default::default()
        };
        let (_, view) = gpui_kit::open_window(options, cx, |_, cx| {
            cx.new(|_| {
                let mut p = Panel {
                    settings: None,
                    snap: Default::default(),
                    price: String::new(),
                    app: AppConfig::default(),
                    error: None,
                };
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
                    let snap = s
                        .as_ref()
                        .map(|s| status::snapshot(s.console_port))
                        .unwrap_or_default();
                    let pr = s
                        .as_ref()
                        .map(|s| price(s.console_port, &snap))
                        .unwrap_or_default();
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

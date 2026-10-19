//! Mesh panel, opened from the tray: every control in one window. The tray
//! owns the engine; the panel sends it actions (`panel_ipc::send`) and shows
//! the state it publishes. Model and price are the app's own preferences in
//! `mesh-app.toml`, applied by the tray.
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use mesh_tray::app_config::{self, AppConfig, Price, FREE, LADDER};
use mesh_tray::panel_ipc::{self, State};
use mesh_tray::{model_selection, settings, settings::Connection};
use std::path::PathBuf;
use std::time::Duration;

const BG: u32 = 0x14161a;
const CARD: u32 = 0x1c1f24;
const MUTED: u32 = 0x8a8f98;
const ACCENT: u32 = 0x2f6fed;
const IDLE: u32 = 0x2a2e35;

struct Panel {
    root: PathBuf,
    settings: settings::Settings,
    state: State,
    app: AppConfig,
    auto: Option<&'static str>,
    error: Option<String>,
    /// Mode just picked (private?), shown at once until saved or cancelled.
    picked: Option<(bool, std::time::Instant)>,
}

impl Panel {
    fn load(root: PathBuf) -> Self {
        let settings = settings::Settings::load(&root).unwrap_or_default();
        let auto = model_selection::local_model(&settings.connection)
            .ok()
            .flatten()
            .and_then(|m| LADDER.iter().find(|r| r.model == m).map(|r| r.model));
        Self {
            state: panel_ipc::read_state(&root).unwrap_or_default(),
            app: AppConfig::load(&root),
            settings,
            auto,
            error: None,
            picked: None,
            root,
        }
    }

    fn pick(&mut self, private: bool, cx: &mut Context<Self>) {
        let saved = matches!(self.settings.connection, Connection::Private { .. });
        if private != saved && self.picked.is_none() {
            self.picked = Some((private, std::time::Instant::now()));
            self.act(if private { "private" } else { "public" }, cx);
        }
    }

    fn act(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if let Err(e) = panel_ipc::send(&self.root, id) {
            self.error = Some(format!("Could not reach Mesh: {e}"));
        }
        cx.notify();
    }

    fn edit(&mut self, f: impl FnOnce(&mut AppConfig), cx: &mut Context<Self>) {
        f(&mut self.app);
        self.error = self
            .app
            .save(&self.root)
            .err()
            .map(|e| format!("Could not save: {e}"));
        cx.notify();
    }

    /// The model being configured: the explicit choice, else the auto pick.
    fn model(&self) -> Option<&'static str> {
        self.app.chosen_model().or(self.auto)
    }
}

fn card(title: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .bg(rgb(CARD))
        .child(div().text_xs().text_color(rgb(MUTED)).child(title))
}

fn chip(id: impl Into<ElementId>, text: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    div()
        .id(id.into())
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(if on { ACCENT } else { IDLE }))
        .hover(|d| d.opacity(0.85))
        .child(text.into())
}

/// A labelled on/off switch row.
fn switch(id: &'static str, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .child(div().child(label.into()))
        .child(
            div()
                .w(px(34.))
                .h(px(18.))
                .rounded_full()
                .p(px(2.))
                .flex()
                .when(on, |d| d.justify_end())
                .bg(rgb(if on { ACCENT } else { IDLE }))
                .child(div().size(px(14.)).rounded_full().bg(rgb(0xffffff))),
        )
}

/// The jellyfish, drawn from the tray's 32×32 RGBA raster as plain boxes:
/// no image decoder or asset source involved, so it always renders.
fn logo() -> Div {
    const N: usize = 32;
    const PX: f32 = 1.5;
    let raw = include_bytes!("../../assets/mesh-jellyfish.rgba");
    let mut d = div().relative().flex_none().size(px(N as f32 * PX));
    for (i, p) in raw.as_chunks::<4>().0.iter().enumerate() {
        if p[3] < 24 {
            continue;
        }
        let (x, y) = ((i % N) as f32, (i / N) as f32);
        let c = (u32::from(p[0]) << 24)
            | (u32::from(p[1]) << 16)
            | (u32::from(p[2]) << 8)
            | u32::from(p[3]);
        d = d.child(
            div()
                .absolute()
                .left(px(x * PX))
                .top(px(y * PX))
                .size(px(PX))
                .bg(rgba(c)),
        );
    }
    d
}

fn muted(text: impl Into<SharedString>) -> Div {
    div().text_xs().text_color(rgb(MUTED)).child(text.into())
}

fn sats(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.clone();
        let saved_private = matches!(self.settings.connection, Connection::Private { .. });
        // Show the pick at once; drop it once saved, or after the confirm
        // dialog has had time to be answered (cancel reverts).
        if let Some((want, at)) = self.picked {
            if (want == saved_private && !st.busy) || at.elapsed() > Duration::from_secs(60) {
                self.picked = None;
            }
        }
        let switching = self.picked.is_some();
        let private = self.picked.map_or(saved_private, |(p, _)| p);
        let rules = self.settings.share_rules.clone();
        let model = self.model();

        let header = div().flex().items_center().gap_3().child(logo()).child(
            div()
                .flex()
                .flex_col()
                .child(div().text_lg().child("Mesh"))
                .child(muted(st.status.clone())),
        );

        let mut network = card("NETWORK").child(
            div()
                .flex()
                .gap_2()
                .child(
                    chip("public", "Public", !private)
                        .on_click(cx.listener(|p, _, _, cx| p.pick(false, cx))),
                )
                .child(
                    chip("private", "Private", private)
                        .on_click(cx.listener(|p, _, _, cx| p.pick(true, cx))),
                )
                .when(switching, |d| d.child(muted("Switching…"))),
        );
        if private && !switching {
            network = network.child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        chip("invite", "Invite someone…", false)
                            .on_click(cx.listener(|p, _, _, cx| p.act("invite", cx))),
                    )
                    .child(
                        chip("join", "Join with an invite…", false)
                            .on_click(cx.listener(|p, _, _, cx| p.act("join", cx))),
                    ),
            );
        }

        let share = card("SHARE")
            .child(
                switch("compute", "Share compute", st.sharing)
                    .on_click(cx.listener(|p, _, _, cx| p.act("compute", cx))),
            )
            .child(muted(if st.busy {
                "Applying…".to_string()
            } else {
                st.share_status.clone()
            }))
            .when(st.sharing, |d| {
                d.child(
                    switch(
                        "spare",
                        "Only when sunny or there's surplus energy",
                        rules.spare_power,
                    )
                    .on_click(cx.listener(|p, _, _, cx| p.act("share:spare", cx))),
                )
                .when(rules.spare_power, |d| {
                    let loc = match rules.location {
                        Some(l) => format!("Location {:.2}, {:.2} · change…", l.lat, l.lon),
                        None => "Location from your IP address · change…".into(),
                    };
                    d.child(
                        div()
                            .id("location")
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .child(loc)
                            .on_click(cx.listener(|p, _, _, cx| p.act("share:location", cx))),
                    )
                })
                .child(
                    switch(
                        "hours",
                        format!("Only between {}", rules.hours.label()),
                        rules.hours_on,
                    )
                    .on_click(cx.listener(|p, _, _, cx| p.act("share:hours", cx))),
                )
            });

        let mut picks = div().flex().flex_wrap().gap_2().child(
            chip(
                "auto",
                format!(
                    "Automatic{}",
                    self.auto
                        .map(|m| format!(" · {}", app_config::label(m)))
                        .unwrap_or_default()
                ),
                self.app.chosen_model().is_none(),
            )
            .on_click(cx.listener(|p, _, _, cx| p.edit(|a| a.model = None, cx))),
        );
        // Only the rungs below the automatic pick: smaller, cheaper options.
        let below = LADDER
            .iter()
            .position(|r| Some(r.model) == self.auto)
            .map_or(0, |i| i + 1);
        for (i, rung) in LADDER.iter().enumerate().skip(below) {
            if !model_selection::fits(rung.model) {
                continue;
            }
            let size = if i + 1 == LADDER.len() {
                "tiny"
            } else {
                "small"
            };
            picks = picks.child(
                chip(
                    ("rung", i),
                    format!("{} ({size})", rung.label),
                    self.app.chosen_model() == Some(rung.model),
                )
                .on_click(cx.listener(move |p, _, _, cx| {
                    p.edit(|a| a.model = Some(LADDER[i].model.to_string()), cx)
                })),
            );
        }
        let model_card = card("MODEL").child(picks).when(model.is_none(), |d| {
            d.child(muted("Too small to share a model"))
        });

        let earn = model.filter(|_| !private).map(|m| {
            let price = self.app.price_for(m);
            let suggested = LADDER.iter().find(|r| r.model == m).map(|r| r.suggested);
            let base = price.or(suggested);
            let set = move |p: &mut Panel, v: Price, cx: &mut Context<Panel>| {
                p.edit(
                    |a| {
                        a.prices.insert(m.to_string(), v);
                    },
                    cx,
                )
            };
            let mut c = card("EARN FOR COMPUTE SHARED").child(
                switch(
                    "earn",
                    format!("Charge for {}", app_config::label(m)),
                    price.is_some(),
                )
                .on_click(cx.listener(move |p, _, _, cx| {
                    let next = if p.app.price_for(m).is_some() {
                        FREE
                    } else {
                        suggested.unwrap_or(Price {
                            input_sats: 100,
                            output_sats: 100,
                        })
                    };
                    set(p, next, cx)
                })),
            );
            c = match price {
                None => c.child(muted("Free")),
                Some(pr) => c
                    .child(div().child(format!(
                        "{} sats in · {} sats out, per 1M tokens",
                        sats(pr.input_sats),
                        sats(pr.output_sats)
                    )))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children([("half", "Lower", 0.5f64), ("double", "Higher", 2.0)].map(
                                |(id, text, f)| {
                                    chip(id, text, false).on_click(cx.listener(
                                        move |p, _, _, cx| {
                                            let Some(b) = base else { return };
                                            let s = |v: u64| ((v as f64 * f).round() as u64).max(1);
                                            set(
                                                p,
                                                Price {
                                                    input_sats: s(b.input_sats),
                                                    output_sats: s(b.output_sats),
                                                },
                                                cx,
                                            )
                                        },
                                    ))
                                },
                            ))
                            .when_some(suggested.filter(|s| Some(*s) != price), |d, sug| {
                                d.child(
                                    chip("suggested", "Suggested", false)
                                        .on_click(cx.listener(move |p, _, _, cx| set(p, sug, cx))),
                                )
                            }),
                    ),
            };
            c
        });

        let pay = (!private).then(|| {
            card("PAY FOR INFERENCE")
                .child(div().child(match (st.wallet, st.balance_sats) {
                    (true, Some(b)) => format!("Wallet balance: {} sats", sats(b)),
                    (true, None) => "Wallet balance: checking…".into(),
                    (false, _) => "No wallet yet. One is set up the first time you need it.".into(),
                }))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            chip("fund", "Add funds (QR)…", false)
                                .on_click(cx.listener(|p, _, _, cx| p.act("pay:fund", cx))),
                        )
                        .child(
                            chip("pay", "Pay for models…", false)
                                .on_click(cx.listener(|p, _, _, cx| p.act("pay:pay", cx))),
                        ),
                )
        });

        let footer = div()
            .flex()
            .gap_2()
            .justify_end()
            .child(
                chip("chat", "Launch Console", false)
                    .on_click(cx.listener(|p, _, _, cx| p.act("chat", cx))),
            )
            .child(
                chip("reset", "Reset…", false)
                    .on_click(cx.listener(|p, _, _, cx| p.act("reset", cx))),
            )
            .child(
                chip("quit", "Quit Mesh", false)
                    .on_click(cx.listener(|p, _, _, cx| p.act("quit", cx))),
            );

        div()
            .id("panel")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .pt(px(36.))
            .bg(rgb(BG))
            .text_color(rgb(0xe6e8eb))
            .text_sm()
            .child(header)
            .child(network)
            .child(share)
            .child(model_card)
            .children(earn)
            .children(pay)
            .when_some(self.error.clone(), |d, e| {
                d.child(div().text_color(rgb(0xff6b6b)).child(e))
            })
            .child(footer)
    }
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        pid != 0 && unsafe { libc::kill(pid as i32, 0) } == 0
    }
    #[cfg(not(unix))]
    {
        pid != 0
    }
}

fn main() {
    let Ok(root) = settings::data_root() else {
        return;
    };
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let bounds = Bounds::centered(None, size(px(520.), px(900.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Mesh".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            focus: true,
            ..Default::default()
        };
        let r = root.clone();
        let (window, view) =
            gpui_kit::open_window(options, cx, move |_, cx| cx.new(|_| Panel::load(r)))
                .expect("open panel window");
        cx.activate(true);
        let view = view.downgrade();
        let mut raised = None;
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let r = root.clone();
            let next = cx
                .background_executor()
                .spawn(async move { Panel::load(r) })
                .await;
            // The tray is gone: the panel has nothing to control.
            if next.state.tray_pid != 0 && !alive(next.state.tray_pid) {
                cx.update(|cx| cx.quit());
                break;
            }
            // Settings was picked while open: come forward (and unminimize).
            if raised.is_some_and(|r| r != next.state.raise) {
                let _ = window.update(cx, |_, w, cx| {
                    cx.activate(true);
                    w.activate_window();
                });
            }
            raised = Some(next.state.raise);
            if view
                .update(cx, |p, cx| {
                    p.settings = next.settings;
                    p.state = next.state;
                    p.app = next.app;
                    p.auto = next.auto;
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

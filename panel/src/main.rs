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
const SUN: u32 = 0xf5b942;
const NIGHT: u32 = 0x22262e;
const DAY: u32 = 0x3a3420;

struct Panel {
    root: PathBuf,
    settings: settings::Settings,
    state: State,
    app: AppConfig,
    auto: Option<&'static str>,
    error: Option<String>,
    /// Mode just picked (private?), shown at once until saved or cancelled.
    picked: Option<(bool, std::time::Instant)>,
    /// "Pricing and wallet" expanded; hidden by default to keep it minimal.
    more: bool,
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
            more: false,
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

    fn act(&mut self, id: &str, cx: &mut Context<Self>) {
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

/// Approximate local sunrise/sunset in minutes since midnight (NOAA
/// declination/hour-angle formula, no equation of time: within ~15 min).
/// `None` in polar day/night.
fn daylight(lat: f64, lon: f64) -> Option<(f64, f64)> {
    use chrono::{Datelike, Offset};
    let now = chrono::Local::now();
    let day = f64::from(now.ordinal());
    let tz = f64::from(now.offset().fix().local_minus_utc()) / 3600.0;
    let decl = (23.44f64).to_radians() * ((360.0 / 365.0) * (day - 81.0)).to_radians().sin();
    let cos_h = -lat.to_radians().tan() * decl.tan();
    if !(-1.0..=1.0).contains(&cos_h) {
        return None;
    }
    let half = cos_h.acos().to_degrees() / 15.0;
    let noon = 12.0 - lon / 15.0 + tz;
    let wrap = |h: f64| (h.rem_euclid(24.0)) * 60.0;
    Some((wrap(noon - half), wrap(noon + half)))
}

fn in_span(m: f64, a: f64, b: f64) -> bool {
    if a <= b {
        (a..b).contains(&m)
    } else {
        m >= a || m < b
    }
}

/// The day at a glance: a 24-hour strip shaded for daylight (when the
/// location is typed), the chosen sharing hours, and the sun (or moon) at the
/// current time riding an arc above it. Explanatory only; the tray decides.
fn day_strip(rules: &mesh_tray::share_rules::ShareRules, sharing_now: bool) -> Div {
    const W: f32 = 456.;
    const ARC: f32 = 34.;
    const SLOTS: usize = 48;
    let now = f64::from(mesh_tray::share_rules::local_minute());
    let sun = rules.location.and_then(|l| daylight(l.lat, l.lon));
    let (rise, set) = sun.unwrap_or((6.0 * 60.0, 18.0 * 60.0));
    let up = in_span(now, rise, set);
    let x = |m: f64| (m / 1440.0) as f32 * W;
    // Height on the arc: a half sine across daylight, below the line at night.
    let lift = |m: f64| {
        let len = (set - rise).rem_euclid(1440.0).max(1.0);
        let t = (m - rise).rem_euclid(1440.0) / len;
        if t <= 1.0 {
            (t * std::f64::consts::PI).sin() as f32
        } else {
            0.0
        }
    };
    let mut arc = div().relative().w(px(W)).h(px(ARC + 12.));
    for i in 0..=36 {
        let m = rise + (set - rise).rem_euclid(1440.0) * f64::from(i) / 36.0;
        arc = arc.child(
            div()
                .absolute()
                .left(px(x(m.rem_euclid(1440.0)) - 1.))
                .top(px(ARC + 6. - lift(m.rem_euclid(1440.0)) * ARC))
                .size(px(2.))
                .rounded_full()
                .bg(rgba(0xf5b94255)),
        );
    }
    let (dot, glow) = if up {
        (SUN, 0xf5b94233)
    } else {
        (0xc9cfd8, 0xc9cfd822)
    };
    let top = ARC + 6. - lift(now) * ARC;
    arc = arc
        .child(
            div()
                .absolute()
                .left(px(x(now) - 9.))
                .top(px(top - 8.))
                .size(px(18.))
                .rounded_full()
                .bg(rgba(glow)),
        )
        .child(
            div()
                .absolute()
                .left(px(x(now) - 5.))
                .top(px(top - 4.))
                .size(px(10.))
                .rounded_full()
                .bg(rgb(dot)),
        );

    let mut bar = div()
        .relative()
        .flex()
        .w(px(W))
        .h(px(10.))
        .rounded_md()
        .overflow_hidden();
    for i in 0..SLOTS {
        let m = (i as f64 + 0.5) * 1440.0 / SLOTS as f64;
        let mut c = if sun.is_some() && in_span(m, rise, set) {
            DAY
        } else {
            NIGHT
        };
        if rules.hours_on && rules.hours.contains(m as u16) {
            c = if sharing_now { ACCENT } else { 0x2a4170 };
        }
        bar = bar.child(div().flex_1().h_full().bg(rgb(c)));
    }
    bar = bar.child(
        div()
            .absolute()
            .left(px(x(now) - 1.))
            .top_0()
            .w(px(2.))
            .h_full()
            .bg(rgb(0xffffff)),
    );

    let ticks = div().flex().justify_between().w(px(W)).children(
        ["00", "06", "12", "18", "24"].map(|t| div().text_xs().text_color(rgb(MUTED)).child(t)),
    );
    let fmt = |m: f64| format!("{:02}:{:02}", (m as u32 / 60) % 24, m as u32 % 60);
    let legend = match sun {
        Some(_) => format!("Sunrise {} · sunset {} (approx.)", fmt(rise), fmt(set)),
        None if rules.location.is_some() => "No sunrise or sunset today".into(),
        None => "Set a location to show daylight".into(),
    };
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(arc)
        .child(bar)
        .child(ticks)
        .child(
            div()
                .flex()
                .gap_3()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(legend)
                .when(rules.hours_on, |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(div().size(px(8.)).rounded_sm().bg(rgb(ACCENT)))
                            .child(format!("Sharing {}", rules.hours.label())),
                    )
                }),
        )
}

fn muted(text: impl Into<SharedString>) -> Div {
    div().text_xs().text_color(rgb(MUTED)).child(text.into())
}

/// " (≈ $1.23)" when a BTC/USD rate is known, else nothing.
fn usd(n: u64, st: &State) -> String {
    st.usd_per_btc_cents
        .map(|c| format!(" (≈ {})", mesh_tray::fx::usd(n, c as f64 / 100.0)))
        .unwrap_or_default()
}

/// Move a minute-of-day by `delta` hours, wrapping at midnight.
fn shift(minute: u16, delta: i32) -> u16 {
    (i32::from(minute) + delta * 60).rem_euclid(1440) as u16
}

/// "From − 10:00 +   To − 15:00 +": steps the sharing window an hour at a time.
fn hours_editor(h: mesh_tray::share_rules::Hours, cx: &mut Context<Panel>) -> Div {
    let fmt = |m: u16| format!("{:02}:{:02}", m / 60, m % 60);
    let stepper = |which: &'static str, value: u16, cx: &mut Context<Panel>| {
        let step = move |delta: i32| {
            move |p: &mut Panel, _: &ClickEvent, _: &mut Window, cx: &mut Context<Panel>| {
                let (mut start, mut end) = (h.start, h.end);
                if which == "start" {
                    start = shift(start, delta);
                } else {
                    end = shift(end, delta);
                }
                if start != end {
                    p.act(&format!("share:hours={}-{}", fmt(start), fmt(end)), cx);
                }
            }
        };
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(muted(if which == "start" { "From" } else { "To" }))
            .child(chip((which, 0usize), "−", false).on_click(cx.listener(step(-1))))
            .child(div().w(px(44.)).flex().justify_center().child(fmt(value)))
            .child(chip((which, 1usize), "+", false).on_click(cx.listener(step(1))))
    };
    div()
        .flex()
        .gap_4()
        .child(stepper("start", h.start, cx))
        .child(stepper("end", h.end, cx))
}

/// Live inference on this node: a quiet card, lit while requests are running.
fn activity_card(a: mesh_tray::status::Activity) -> Div {
    let live = a.active > 0;
    let speed = a
        .tps_tenths
        .map(|t| format!(" · {}.{} tok/s", t / 10, t % 10))
        .unwrap_or_default();
    let now = if live {
        format!(
            "Running {} request{} now{speed}",
            a.active,
            if a.active == 1 { "" } else { "s" }
        )
    } else {
        format!("Idle{speed}")
    };
    card("ACTIVITY")
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().size(px(8.)).rounded_full().bg(rgb(if live {
                    0x3ccf7a
                } else {
                    IDLE
                })))
                .child(now),
        )
        .child(muted(format!(
            "{} tokens generated · {} requests done since Mesh started",
            sats(a.output_tokens),
            sats(a.completed)
        )))
        .child(muted(format!(
            "{} other node{} in the Mesh, {} sharing a model",
            a.peers,
            if a.peers == 1 { "" } else { "s" },
            a.serving_peers
        )))
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
            .when(st.sharing && rules.any(), |d| {
                d.child(day_strip(&rules, !st.share_status.starts_with("Paused")))
            })
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
                .when(rules.hours_on, |d| d.child(hours_editor(rules.hours, cx)))
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
                        "{} sats in{} · {} sats out{}, per 1M tokens",
                        sats(pr.input_sats),
                        usd(pr.input_sats, &st),
                        sats(pr.output_sats),
                        usd(pr.output_sats, &st)
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
                    (true, Some(b)) => format!("Wallet balance: {} sats{}", sats(b), usd(b, &st)),
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
            .items_center()
            .child(
                div()
                    .id("console")
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|d| d.underline())
                    .child("Console")
                    .on_click(cx.listener(|p, _, _, cx| p.act("console", cx))),
            )
            .child(
                chip("chat", "Chat", true)
                    .px_5()
                    .py_2()
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
            .when_some(st.activity, |d, a| d.child(activity_card(a)))
            .child(model_card)
            .when(earn.is_some() || pay.is_some(), |d| {
                d.child(
                    div()
                        .id("more")
                        .flex()
                        .justify_between()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .hover(|d| d.text_color(rgb(0xe6e8eb)))
                        .child("PRICING AND WALLET")
                        .child(if self.more { "Hide ▴" } else { "Show ▾" })
                        .on_click(cx.listener(|p, _, _, cx| {
                            p.more = !p.more;
                            cx.notify();
                        })),
                )
            })
            .when(self.more, |d| d.children(earn).children(pay))
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

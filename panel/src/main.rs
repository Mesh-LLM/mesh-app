//! Mesh panel, opened from the tray: every control in one window. The tray
//! owns the engine; the panel sends it actions (`panel_ipc::send`) and shows
//! the state it publishes. Model and price are the app's own preferences in
//! `mesh-app.toml`, applied by the tray.
use gpui_kit::component::{
    button::{Button, ButtonGroup, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
    Disableable, Selectable, Sizable,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use mesh_tray::app_config::{self, AppConfig, Price, FREE, LADDER};
use mesh_tray::panel_ipc::{self, State};
use mesh_tray::settings;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy)]
struct Palette {
    bg: u32,
    card: u32,
    muted: u32,
    accent: u32,
    idle: u32,
    sun: u32,
    night: u32,
    day: u32,
    text: u32,
}
impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                bg: 0x14161a,
                card: 0x1c1f24,
                muted: 0x9a9fa8,
                accent: 0x2f6fed,
                idle: 0x2a2e35,
                sun: 0xf5b942,
                night: 0x22262e,
                day: 0x3a3420,
                text: 0xe6e8eb,
            }
        } else {
            Self {
                bg: 0xf4f5f7,
                card: 0xffffff,
                muted: 0x59616e,
                accent: 0x245edb,
                idle: 0xd4d8df,
                sun: 0x976800,
                night: 0xe5e8ee,
                day: 0xf4e7bf,
                text: 0x20242b,
            }
        }
    }
}

struct Panel {
    root: PathBuf,
    state: State,
    app: AppConfig,
    auto: Option<&'static str>,
    error: Option<String>,
    /// Mode just picked (private?), shown at once until saved or cancelled.
    picked: Option<(bool, u64)>,
    preview: bool,
    /// "Pricing and wallet" expanded; hidden by default to keep it minimal.
    more: bool,
    /// "Use with your agent" expanded.
    agent: bool,
    /// "Activity" and "Advanced" expanded.
    activity: bool,
    advanced: bool,
    /// Requests-done count last seen and when it went up: drives "SERVED".
    served: (u64, Option<std::time::Instant>),
    /// What was last copied, shown as a confirmation for a few seconds.
    copied: Option<(&'static str, std::time::Instant)>,
}

impl Panel {
    fn preview() -> Self {
        let mut state = State {
            status: "Connected · ready to chat".into(),
            public: true,
            sharing: true,
            share_status: "Sharing enabled · serving Gemma 4 12B".into(),
            serving: Some("Gemma 4 12B".into()),
            activity: Some(mesh_tray::status::Activity::default()),
            api_port: 9447,
            automatic_model: Some(LADDER[1].model.into()),
            available_models: LADDER[1..].iter().map(|r| r.model.into()).collect(),
            ..Default::default()
        };
        if std::env::var("MESH_PANEL_PREVIEW_LONG_STATUS").as_deref() == Ok("1") {
            state.status = "Mesh is still stopping. App remains open and will finish the requested change when shutdown completes. No other processes were stopped.".into();
        }
        let conditions = std::env::var("MESH_PANEL_PREVIEW_CONDITIONS").as_deref() == Ok("1");
        if conditions {
            state.sharing = false;
            state.serving = None;
            state.share_status = "Sharing is off".into();
        }
        state
            .preferences
            .prices
            .insert(LADDER[1].model.into(), FREE);
        Self {
            root: PathBuf::new(),
            app: state.preferences.clone(),
            state,
            auto: Some(LADDER[1].model),
            error: Some("Read-only preview · no live Mesh connection".into()),
            picked: None,
            preview: true,
            more: false,
            agent: false,
            activity: true,
            advanced: false,
            served: (0, None),
            copied: None,
        }
    }

    fn load(root: PathBuf) -> Self {
        let state = panel_ipc::read_state(&root).unwrap_or_default();
        let auto = state
            .automatic_model
            .as_deref()
            .and_then(|m| LADDER.iter().find(|r| r.model == m).map(|r| r.model));
        Self {
            app: state.preferences.clone(),
            state,
            preview: false,
            auto,
            error: None,
            picked: None,
            more: false,
            agent: false,
            activity: true,
            advanced: false,
            served: (0, None),
            copied: None,
            root,
        }
    }

    fn pick(&mut self, private: bool, cx: &mut Context<Self>) {
        if self.preview {
            self.act("preview", cx);
            return;
        }
        let saved = !self.state.public;
        if private != saved && self.picked.is_none() {
            self.picked = Some((private, self.state.command_revision));
            if private {
                self.activity = true;
            }
            self.act(if private { "private" } else { "public" }, cx);
        }
    }

    fn act(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.preview {
            self.error = Some("Preview only — no Mesh settings are changed.".into());
            cx.notify();
            return;
        }
        if let Err(e) = panel_ipc::send(&self.root, id) {
            self.error = Some(format!("Could not reach Mesh: {e}"));
        }
        cx.notify();
    }

    fn edit(&mut self, f: impl FnOnce(&mut AppConfig), cx: &mut Context<Self>) {
        if self.preview || self.state.busy {
            self.error = Some(
                "Preview only or Mesh is applying a change; preferences were not saved.".into(),
            );
            cx.notify();
            return;
        }
        let mut next = self.app.clone();
        f(&mut next);
        let action = if next.model != self.app.model {
            Some(panel_ipc::Action::SetModel(next.model))
        } else {
            next.prices
                .iter()
                .find(|(m, p)| self.app.prices.get(*m) != Some(*p))
                .map(|(m, p)| panel_ipc::Action::SetPrice {
                    model: m.clone(),
                    price: *p,
                })
        };
        if let Some(action) = action {
            self.error = panel_ipc::send_action(&self.root, &action).err();
        }
        cx.notify();
    }

    fn copy(&mut self, what: &'static str, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = Some((what, std::time::Instant::now()));
        cx.notify();
    }

    /// The model being configured: the explicit choice, else the auto pick.
    fn model(&self) -> Option<&'static str> {
        self.app
            .model
            .as_deref()
            .and_then(|m| LADDER.iter().find(|r| r.model == m).map(|r| r.model))
            .or(self.auto)
    }
}

/// Keep routine on/off state on the switch; only explain exceptions.
fn sharing_notice(state: &State) -> Option<String> {
    if state.busy {
        return None;
    }
    if !state.sharing {
        return None;
    }
    let text = state.share_status.as_str();
    if text.is_empty()
        || matches!(text, "Sharing now" | "Sharing whenever Mesh runs")
        || text.starts_with("Sharing enabled · serving")
    {
        None
    } else {
        Some(text.into())
    }
}

/// Automatic is always first. Keep the current explicit choice visible even
/// when it is no longer eligible, so the selector never misrepresents settings.
fn model_choices(state: &State) -> Vec<(Option<String>, String)> {
    let mut choices = vec![(None, "Automatic (default)".into())];
    for rung in LADDER {
        if state.available_models.iter().any(|m| m == rung.model)
            || state.preferences.model.as_deref() == Some(rung.model)
        {
            choices.push((Some(rung.model.into()), rung.label.into()));
        }
    }
    if let Some(model) = &state.preferences.model {
        if !choices.iter().any(|(m, _)| m.as_ref() == Some(model)) {
            choices.push((Some(model.clone()), model.clone()));
        }
    }
    choices
}

fn card(palette: Palette, title: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .bg(rgb(palette.card))
        .when(!title.is_empty(), |d| {
            d.child(div().text_xs().text_color(rgb(palette.muted)).child(title))
        })
}

fn chip(id: impl Into<ElementId>, text: impl Into<SharedString>, on: bool) -> Button {
    Button::new(id)
        .label(text)
        .selected(on)
        .when(on, |b| b.primary())
        .when(!on, |b| b.ghost())
}

fn switch(id: &'static str, label: impl Into<SharedString>, on: bool) -> Switch {
    Switch::new(id).label(label.into()).checked(on)
}

fn logo() -> Div {
    static IMAGE: std::sync::OnceLock<std::sync::Arc<RenderImage>> = std::sync::OnceLock::new();
    let image = IMAGE.get_or_init(|| {
        let mut bgra = include_bytes!("../../assets/mesh-jellyfish.rgba").to_vec();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let buffer = image::RgbaImage::from_raw(32, 32, bgra).expect("bundled 32px logo");
        std::sync::Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
    });
    div()
        .flex_none()
        .size(px(48.))
        .child(img(image.clone()).size_full())
}

fn accent_theme(cx: &mut App) {
    use gpui_kit::component::Theme;
    Theme::update(cx, |theme| {
        theme.primary = rgb(0x2f6fed).into();
        theme.primary_hover = rgb(0x245edb).into();
        theme.primary_active = rgb(0x1d4fc0).into();
        theme.primary_foreground = rgb(0xffffff).into();
        theme.button_primary = theme.primary;
        theme.button_primary_hover = theme.primary_hover;
        theme.button_primary_active = theme.primary_active;
        theme.button_primary_foreground = theme.primary_foreground;
        theme.switch = theme.primary;
    });
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
fn day_strip(
    palette: Palette,
    rules: &mesh_tray::share_rules::ShareRules,
    sharing_now: bool,
) -> Div {
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
        (palette.sun, 0xf5b94233)
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
            palette.day
        } else {
            palette.night
        };
        if rules.hours_on && rules.hours.contains(m as u16) {
            c = if sharing_now {
                palette.accent
            } else {
                0x2a4170
            };
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
        ["00", "06", "12", "18", "24"]
            .map(|t| div().text_xs().text_color(rgb(palette.muted)).child(t)),
    );
    let fmt = |m: f64| format!("{:02}:{:02}", (m as u32 / 60) % 24, m as u32 % 60);
    let legend = match sun {
        Some(_) => format!("Sunrise {} · sunset {} (approx.)", fmt(rise), fmt(set)),
        None if rules.location.is_some() => "No sunrise or sunset today".into(),
        None => "Approximate daylight".into(),
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
                .text_color(rgb(palette.muted))
                .child(legend)
                .when(rules.hours_on, |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(div().size(px(8.)).rounded_sm().bg(rgb(palette.accent)))
                            .child(format!("Sharing {}", rules.hours.label())),
                    )
                }),
        )
}

fn muted(palette: Palette, text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .text_color(rgb(palette.muted))
        .child(text.into())
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
fn hours_editor(
    palette: Palette,
    h: mesh_tray::share_rules::Hours,
    cx: &mut Context<Panel>,
) -> Div {
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
            .child(muted(palette, if which == "start" { "From" } else { "To" }))
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

/// A collapsible section header ("TITLE   summary   Show ▾").
fn fold(
    palette: Palette,
    id: &'static str,
    title: &'static str,
    open: bool,
    summary: Option<Div>,
) -> Button {
    Button::new(id)
        .ghost()
        .w_full()
        .accessibility_label(format!(
            "{title}, {}",
            if open { "expanded" } else { "collapsed" }
        ))
        .child(
            div()
                .flex()
                .w_full()
                .items_center()
                .gap_2()
                .text_color(rgb(palette.muted))
                .child(title)
                .child(div().flex_1().children(summary))
                .child(if open { "Hide ▴" } else { "Show ▾" }),
        )
}

/// 3×5 dot font for the activity display (after the Buzz compute widget).
fn glyph(c: char) -> &'static str {
    match c {
        'D' => "110101101101110",
        'E' => "111100110100111",
        'G' => "111100101101111",
        'I' => "111010010010111",
        'K' => "101101110101101",
        'L' => "100100100100111",
        'N' => "110101101101101",
        'O' => "111101101101111",
        'R' => "110101110101101",
        'S' => "111100111001111",
        'V' => "101101101101010",
        'W' => "101101101111111",
        _ => "000000000000000",
    }
}

/// Seconds since the panel started, for animation phase.
fn clock() -> f32 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f32()
}

/// A small dot-matrix display: the word in lit dots on a faint grid. While
/// `live`, a bright band sweeps across the letters and the grid twinkles;
/// otherwise it sits still (no motion that implies work is happening).
fn dot_matrix(word: &str, color: u32, background: u32, live: bool) -> Div {
    const COLS: usize = 33;
    const ROWS: usize = 7;
    let mut lit = [[false; COLS]; ROWS];
    let width = (word.chars().count() * 4).saturating_sub(1);
    let x0 = COLS.saturating_sub(width) / 2;
    for (i, c) in word.chars().enumerate() {
        for (k, b) in glyph(c).bytes().enumerate() {
            let (x, y) = (x0 + i * 4 + k % 3, 1 + k / 3);
            if b == b'1' && x < COLS {
                lit[y][x] = true;
            }
        }
    }
    let t = clock();
    let sweep = (t * 14.0) % (COLS as f32 + 12.0) - 6.0;
    let (r, g, b) = (
        (color >> 16) as f32,
        ((color >> 8) & 0xff) as f32,
        (color & 0xff) as f32,
    );
    let dot = |x: usize, y: usize| -> u32 {
        let a = if lit[y][x] {
            if live {
                let d = (x as f32 - sweep).abs();
                0.55 + 0.45 * (1.0 - (d / 4.0).min(1.0))
            } else {
                0.8
            }
        } else if live {
            // Cheap per-dot hash: a slow, sparse twinkle on the background.
            let h = ((x * 73 + y * 151) % 97) as f32;
            let w = ((t * 1.3 + h).sin() * 0.5 + 0.5).powi(12);
            0.07 + 0.25 * w
        } else {
            0.07
        };
        let mix = |c: f32, bg: f32| (bg + (c - bg) * a) as u32;
        (mix(r, (background >> 16) as f32) << 16)
            | (mix(g, ((background >> 8) & 0xff) as f32) << 8)
            | mix(b, (background & 0xff) as f32)
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(2.))
        .py_1()
        .children((0..ROWS).map(|y| {
            div()
                .flex()
                .gap(px(2.))
                .children((0..COLS).map(|x| div().size(px(3.)).rounded_full().bg(rgb(dot(x, y)))))
        }))
}

/// Live inference on this node: a quiet card, lit while requests are running.
fn activity_card(palette: Palette, a: mesh_tray::status::Activity, served: bool) -> Div {
    let live = a.active > 0;
    let (word, color) = if live {
        ("WORKING", 0x3ccf7a)
    } else if served {
        ("SERVED", 0x5aa0ff)
    } else {
        ("IDLE", palette.muted)
    };
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
    card(palette, "")
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(dot_matrix(word, color, palette.card, live || served))
                .child(now),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_x_3()
                .gap_y_1()
                .child(muted(
                    palette,
                    format!(
                        "{} tokens · {} requests since launch",
                        sats(a.output_tokens),
                        sats(a.completed)
                    ),
                ))
                .child(muted(
                    palette,
                    format!("{} peers · {} sharing", a.peers, a.serving_peers),
                )),
        )
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = match std::env::var("MESH_PANEL_PREVIEW_THEME")
            .ok()
            .as_deref()
            .filter(|_| self.preview)
        {
            Some("light") => false,
            Some("dark") => true,
            _ => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        };
        let palette = Palette::new(dark);
        let st = self.state.clone();
        if let Some(a) = st.activity {
            // First reading only sets the baseline, so opening doesn't flash.
            if self.served.1.is_none() {
                self.served = (
                    a.completed,
                    Some(std::time::Instant::now() - Duration::from_secs(60)),
                );
            } else if a.completed > self.served.0 {
                self.served = (a.completed, Some(std::time::Instant::now()));
            }
        }
        let served = self
            .served
            .1
            .is_some_and(|at| at.elapsed() < Duration::from_secs(3));
        let live = st.activity.is_some_and(|a| a.active > 0);
        if self.activity && (live || served) {
            window.request_animation_frame();
        }
        let saved_private = !self.state.public;
        // Show the pick at once; drop it once saved, or after the confirm
        // dialog has had time to be answered (cancel reverts).
        if let Some((_, revision)) = self.picked {
            if st.command_revision != revision {
                self.picked = None;
            }
        }
        let switching = self.picked.is_some();
        let private = self.picked.map_or(saved_private, |(p, _)| p);
        let rules = st.rules.clone();
        let model = self.model();

        let header = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_3()
            .child(logo())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_lg().child("Mesh"))
                    .min_w_0()
                    .flex_1()
                    .child(muted(palette, st.status.clone()).w_full().truncate()),
            );

        let mut network = card(palette, "").child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child("Network")
                .child(
                    ButtonGroup::new("network-mode")
                        .bg(rgb(palette.idle))
                        .rounded_md()
                        .compact()
                        .small()
                        .disabled(st.busy || switching)
                        .child(chip("public", "Public", !private))
                        .child(chip("private", "Private", private))
                        .on_click(cx.listener(|p, selected: &Vec<usize>, _, cx| {
                            if let Some(index) = selected.first() {
                                p.pick(*index == 1, cx);
                            }
                        })),
                )
                .when(switching, |d| d.child(muted(palette, "Switching…"))),
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

        let share = card(palette, "")
            .child(
                switch("compute", "Share compute", st.sharing)
                    .disabled(st.busy)
                    .on_click(cx.listener(|p, _, _, cx| p.act("compute", cx))),
            )
            .when_some(sharing_notice(&st), |d, notice| {
                d.child(muted(palette, notice))
            })
            .child(day_strip(
                palette,
                &rules,
                st.sharing && !st.share_status.starts_with("Paused"),
            ))
            .child(
                switch("spare", "Share only when sunny", rules.spare_power)
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
                        .text_color(rgb(palette.accent))
                        .child(loc)
                        .on_click(cx.listener(|p, _, _, cx| p.act("share:location", cx))),
                )
            })
            .child(
                switch(
                    "hours",
                    format!("Share between {}", rules.hours.label()),
                    rules.hours_on,
                )
                .on_click(cx.listener(|p, _, _, cx| p.act("share:hours", cx))),
            )
            .when(rules.hours_on, |d| {
                d.child(hours_editor(palette, rules.hours, cx))
            });

        let choices = model_choices(&st);
        let selected = self.app.model.clone();
        let label = selected
            .as_deref()
            .map(app_config::label)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "Automatic{}",
                    self.auto
                        .map(|m| format!(" · {}", app_config::label(m)))
                        .unwrap_or_default()
                )
            });
        let panel = cx.entity().downgrade();
        let model_card = card(palette, "")
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child("Model")
                    .child(
                        Button::new("model-picker")
                            .ghost()
                            .small()
                            .label(label)
                            .icon(gpui_kit::component::IconName::ChevronDown)
                            .accessibility_label("Model, choose automatic or a specific model")
                            .disabled(st.busy)
                            .dropdown_menu(move |mut menu, _, _| {
                                for (value, label) in &choices {
                                    let panel = panel.clone();
                                    let value = value.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(label.clone())
                                            .checked(value == selected)
                                            .on_click(move |_, _, cx| {
                                                let _ = panel.update(cx, |p, cx| {
                                                    p.edit(|a| a.model = value.clone(), cx)
                                                });
                                            }),
                                    );
                                }
                                menu
                            }),
                    ),
            )
            .when(model.is_none(), |d| {
                d.child(muted(palette, "Too small to share a model"))
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
            let mut c = card(palette, "Earn for compute shared").child(
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
                None => c.child(muted(palette, "Free")),
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
            card(palette, "Pay for inference")
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

        let api_port = st.api_port;
        let model_id = st.model_id.clone();
        let copied = self
            .copied
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(4))
            .map(|(w, _)| w);
        let agent_card = card(palette, "Local API")
            .child(div().child(format!(
                "OpenAI-compatible API: {}",
                mesh_tray::agent_setup::endpoint(api_port)
            )))
            .child(muted(palette, match &st.serving {
                Some(m) => format!("Model on this machine: {m}. No API key needed."),
                None => "Models from the Mesh are listed at /v1/models. No API key needed.".into(),
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .when(cfg!(target_os = "macos"), |d| d.child(
                        chip("launch-goose", "Open Goose…", true)
                            .on_click(cx.listener(|p, _, _, cx| p.act("goose", cx))),
                    ))
                    .child(chip("copy-prompt", "Copy setup prompt", false).on_click(cx.listener({
                        let model_id = model_id.clone();
                        move |p, _, _, cx| {
                            let text = mesh_tray::agent_setup::prompt(api_port, model_id.as_deref());
                            p.copy("prompt", text, cx)
                        }
                    })))
                    .child(chip("copy-url", "Copy endpoint", false).on_click(cx.listener(
                        move |p, _, _, cx| {
                            p.copy("endpoint", mesh_tray::agent_setup::endpoint(api_port), cx)
                        },
                    ))),
            )
            .child(muted(palette, match copied {
                Some(w) => format!("Copied {w}."),
                None => "Paste the prompt into goose, pi or any agent: it sets itself up, keeps your existing providers, and checks with one request.".into(),
            }));

        let content = div()
            .id("panel")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(palette.bg))
            .text_color(rgb(palette.text))
            .text_sm()
            .child(network)
            .child(share)
            .child(model_card)
            .when_some(st.activity, |d, a| {
                let summary = div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().size(px(6.)).rounded_full().bg(rgb(if live { 0x3ccf7a } else { palette.idle })))
                    .child(if live {
                        format!("{} running", a.active)
                    } else {
                        "Idle".into()
                    });
                d.child(fold(palette, "activity", "Activity", self.activity, Some(summary)).on_click(
                    cx.listener(|p, _, _, cx| {
                        p.activity = !p.activity;
                        cx.notify();
                    }),
                ))
                .when(self.activity, |d| d.child(activity_card(palette, a, served)))
            })
            .child(
                fold(palette, "agent", "Use with your agent", self.agent, None)
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.agent = !p.agent;
                        cx.notify();
                    })),
            )
            .when(self.agent, |d| d.child(agent_card))
            .when(earn.is_some() || pay.is_some(), |d| {
                d.child(
                    fold(palette, "more", "Pricing and wallet", self.more, None)
                        .on_click(cx.listener(|p, _, _, cx| {
                            p.more = !p.more;
                            cx.notify();
                        })),
                )
            })
            .when(self.more, |d| d.children(earn).children(pay))
            .child(fold(palette, "advanced", "Advanced", self.advanced, None).on_click(cx.listener(
                |p, _, _, cx| {
                    p.advanced = !p.advanced;
                    cx.notify();
                },
            )))
            .when(self.advanced, |d| {
                d.child(
                    card(palette, "Advanced")
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(chip("console", "Open console", false).on_click(
                                    cx.listener(|p, _, _, cx| p.act("console", cx)),
                                ))
                                .child(chip("reset", "Reset…", false).on_click(
                                    cx.listener(|p, _, _, cx| p.act("reset", cx)),
                                )),
                        )
                        .child(muted(palette,
                            "Console opens the engine web console. Reset restores default settings (it asks first; wallet funds and models are kept).",
                        )),
                )
            })
            .when_some(self.error.clone().or(st.error), |d, e| {
                d.child(div().text_color(rgb(if self.preview { palette.muted } else if dark { 0xff8585 } else { 0xb42318 })).child(e))
            })
            .into_any_element();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(palette.bg))
            .text_color(rgb(palette.text))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .pt(px(36.))
                    .child(header)
                    .child(
                        chip("chat", "Chat", true)
                            .flex_shrink_0()
                            .on_click(cx.listener(|p, _, _, cx| p.act("chat", cx))),
                    ),
            )
            .child(content)
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
    #[cfg(all(feature = "visual-test", target_os = "macos"))]
    if let Some(output) =
        std::env::args().find_map(|a| a.strip_prefix("--capture=").map(str::to_owned))
    {
        capture_preview(&output);
        return;
    }

    // A read-only visual preview never reads the user's profile or starts an engine.
    let preview = std::env::args().any(|a| a == "--preview");
    let root = if preview {
        PathBuf::new()
    } else {
        let Ok(root) = settings::data_root() else {
            return;
        };
        root
    };
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let bounds = Bounds::centered(None, size(px(520.), px(800.)), cx);
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
        let (window, view) = gpui_kit::open_window(options, cx, move |w, cx| {
            use gpui_kit::component::{Theme, ThemeMode};
            Theme::sync_system_appearance(Some(w), cx);
            accent_theme(cx);
            if preview {
                match std::env::var("MESH_PANEL_PREVIEW_THEME").ok().as_deref() {
                    Some("light") => Theme::change(ThemeMode::Light, Some(w), cx),
                    Some("dark") => Theme::change(ThemeMode::Dark, Some(w), cx),
                    _ => (),
                }
            }
            accent_theme(cx);
            w.observe_window_appearance(|w, cx| {
                Theme::sync_system_appearance(Some(w), cx);
                accent_theme(cx);
                cx.refresh_windows();
            })
            .detach();
            cx.new(|_| {
                if preview {
                    Panel::preview()
                } else {
                    Panel::load(r)
                }
            })
        })
        .expect("open panel window");
        cx.activate(true);
        if preview {
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            return;
        }
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
                    let changed = p.state != next.state;
                    p.state = next.state;
                    p.app = next.app;
                    p.auto = next.auto;
                    if changed || p.copied.is_some() {
                        cx.notify();
                    }
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

/// Uses the real Metal renderer without screen-recording permission or a live engine.
#[cfg(all(feature = "visual-test", target_os = "macos"))]
fn capture_preview(output: &str) {
    use gpui_kit::{
        assets::Assets,
        component::{Theme, ThemeMode},
    };
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        std::sync::Arc::new(Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    let dark = std::env::var("MESH_PANEL_PREVIEW_THEME").as_deref() == Ok("dark");
    cx.update(|cx| {
        Theme::change(
            if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            None,
            cx,
        )
    });
    cx.update(accent_theme);
    let (handle, view) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(520.), px(800.)),
                    })),
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Panel::preview()),
            )
        })
        .unwrap();
    let _ = cx.update_window(handle, |_, w, cx| w.draw(cx)).unwrap();
    cx.capture_screenshot(handle)
        .expect("Metal preview rendering")
        .save(output)
        .unwrap();
    let _ = view;
}

#[cfg(test)]
mod tests {
    use super::{model_choices, sharing_notice, State, LADDER};

    #[test]
    fn sharing_only_explains_exceptional_states() {
        let mut state = State {
            sharing: false,
            share_status: "Sharing is off".into(),
            ..Default::default()
        };
        assert_eq!(sharing_notice(&state), None);
        state.sharing = true;
        for status in ["Sharing now", "Sharing whenever Mesh runs", ""] {
            state.share_status = status.into();
            assert_eq!(sharing_notice(&state), None);
        }
        for status in [
            "Paused: outside 10:00–15:00",
            "Sharing: sunshine unknown (checking weather)",
        ] {
            state.share_status = status.into();
            assert_eq!(sharing_notice(&state).as_deref(), Some(status));
        }
        state.busy = true;
        assert_eq!(sharing_notice(&state), None);
        state.sharing = false;
        assert_eq!(sharing_notice(&state), None);
    }

    #[test]
    fn model_menu_has_default_first_and_preserves_explicit_choice() {
        let mut state = State {
            available_models: vec![LADDER[1].model.into()],
            ..Default::default()
        };
        let choices = model_choices(&state);
        assert_eq!(choices[0], (None, "Automatic (default)".into()));
        assert_eq!(choices.len(), 2);
        state.preferences.model = Some(LADDER[0].model.into());
        assert_eq!(model_choices(&state).len(), 3);
        state.preferences.model = Some("custom/model".into());
        assert_eq!(
            model_choices(&state).last().unwrap().0.as_deref(),
            Some("custom/model")
        );
    }
}

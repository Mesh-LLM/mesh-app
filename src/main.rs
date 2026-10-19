#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use mesh_tray::{identity, settings};
mod invites;
use mesh_tray::lifecycle;
#[cfg(target_os = "macos")]
mod native;
#[cfg(not(target_os = "macos"))]
#[path = "native_portable.rs"]
mod native;
mod pay_menu;
use mesh_tray::payment_reset;
mod qr;
mod settings_actions;
mod share_menu;
mod status;

use lifecycle::Engine;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const POLL: Duration = Duration::from_secs(3);

struct Ui {
    /// What the engine is doing right now, e.g. "Loading Qwen3.8-27B…".
    status: MenuItem,
    _tray: TrayIcon,
}

struct App {
    settings: settings::Settings,
    pending_settings: Option<settings::Settings>,
    resetting: bool,
    retire_private: bool,
    root: PathBuf,
    ui: Option<Ui>,
    child: Option<Engine>,
    rx: Receiver<status::Snapshot>,
    tx: Sender<u16>,
    snapshot: status::Snapshot,
    next_poll: Instant,
    polling: bool,
    started: Option<Instant>,
    stopping: Option<Instant>,
    restart_at: Option<Instant>,
    restart_used: bool,
    error: Option<String>,
    /// Log length when the current run was started, so a fatal line from an
    /// earlier run is never quoted as this run's reason.
    log_mark: u64,
    open_when_ready: Option<&'static str>,
    exit: bool,
    pay: pay_menu::Payments,
    share: share_menu::ShareControl,
    /// Whether the running engine was started serving.
    spawned_share: bool,
    /// Model the running engine was started with.
    spawned_model: Option<String>,
    /// Panel-edited preferences (`mesh-app.toml`), re-read every poll.
    app: mesh_tray::app_config::AppConfig,
    /// (pid, model, price) last pushed to the engine.
    applied_price: Option<(u32, String, Option<mesh_tray::app_config::Price>)>,
    published: Option<mesh_tray::panel_ipc::State>,
    /// Settings clicks while the panel is open (see `panel_ipc::State::raise`).
    raise: u64,
}

fn icon() -> Icon {
    // Mesh's existing jellyfish artwork, rasterized at tray resolution.
    // macOS uses its alpha silhouette as a template on light and dark menu bars.
    Icon::from_rgba(
        include_bytes!("../assets/mesh-jellyfish.rgba").to_vec(),
        32,
        32,
    )
    .expect("valid bundled jellyfish icon")
}

#[cfg(test)]
mod icon_tests {
    #[test]
    fn bundled_jellyfish_has_transparency_and_visible_pixels() {
        super::icon();
        let rgba = include_bytes!("../assets/mesh-jellyfish.rgba");
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert!(rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] == 0));
        assert!(rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] == 255));
    }
}

impl App {
    fn new(root: PathBuf, settings: settings::Settings) -> Self {
        let (tx, jobs) = mpsc::channel();
        let (results, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(port) = jobs.recv() {
                if results.send(status::snapshot(port)).is_err() {
                    break;
                }
            }
        });
        let share = share_menu::ShareControl::new(&settings.share_rules);
        Self {
            share,
            spawned_share: false,
            spawned_model: None,
            app: mesh_tray::app_config::AppConfig::load(&root),
            applied_price: None,
            published: None,
            raise: 0,
            root,
            settings,
            pending_settings: None,
            resetting: false,
            retire_private: false,
            ui: None,
            child: None,
            rx,
            tx,
            snapshot: status::Snapshot::default(),
            next_poll: Instant::now(),
            polling: false,
            started: None,
            stopping: None,
            restart_at: None,
            restart_used: false,
            error: None,
            log_mark: 0,
            open_when_ready: None,
            exit: false,
            // Side-effect free file check; the wallet itself is Mesh's.
            pay: pay_menu::Payments::new(settings::mesh_profile().ok()),
        }
    }

    fn build(&mut self) -> Result<(), String> {
        let menu = Menu::new();
        let chat = MenuItem::with_id("chat", "Open Chat…", true, None);
        let panel = MenuItem::with_id("panel", "Settings…", true, None);
        let quit = MenuItem::with_id("quit", "Quit Mesh", true, None);
        // Pay and share-rule state still own their (unshown) menu items.
        let _ = self.pay.submenu()?;
        let rules = self.settings.share_rules.clone();
        let _ = self.share.submenu(&rules)?;
        let status = MenuItem::with_id("status", "Mesh is starting…", false, None);
        // Every control lives in the panel; the tray menu stays minimal.
        menu.append_items(&[
            &status,
            &PredefinedMenuItem::separator(),
            &panel,
            &chat,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_icon(icon())
            .with_icon_as_template(true)
            .with_tooltip("Mesh")
            .build()
            .map_err(|e| e.to_string())?;
        self.ui = Some(Ui {
            status,
            _tray: tray,
        });
        self.start();
        let _ = open_panel();
        Ok(())
    }

    fn start(&mut self) {
        if self.child.is_some() {
            return;
        }
        self.restart_at = None;
        self.error = None;
        self.log_mark = std::fs::metadata(self.root.join("mesh.log"))
            .map(|m| m.len())
            .unwrap_or(0);
        match self.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.started = Some(Instant::now());
                self.spawned_share = self.effective_share();
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn spawn(&mut self) -> Result<Engine, String> {
        for port in [self.settings.console_port, self.settings.api_port] {
            if std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_millis(100),
            )
            .is_ok()
            {
                return Err(format!(
                    "Port {port} is already in use. Existing service left unchanged."
                ));
            }
        }
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        // One machine identity, shared with the CLI and Buzz. Private verifies
        // it exists without unlocking it; the runtime child unlocks the key, so
        // the user sees one credential prompt, not two.
        let profile = settings::mesh_profile()?;
        if let Ok(true) = mesh_tray::wallet_setup::migrate_builtin_wallet(&profile) {
            eprintln!("Copied the built-in Lexe wallet to the installed wallet plugin's folder");
        }
        if mesh_tray::private_reset::pending(&self.root) {
            return Err("An interrupted reset must finish first. Choose Reset settings.".into());
        }
        if matches!(
            self.settings.connection,
            settings::Connection::Private { .. }
        ) {
            identity::establish(&profile)?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("mesh.log"))
            .map_err(|e| e.to_string())?;
        {
            use std::io::Write;
            // First run on a machine with no engine config gets one, with
            // thinking off for tray chat. An existing config is the user's and
            // is reported, never replaced.
            match mesh_tray::runtime_config::ensure_first_run(&profile)? {
                mesh_tray::runtime_config::Config::Created(path) => {
                    writeln!(
                        &log,
                        "Tray wrote first-run engine config: {}",
                        path.display()
                    )
                }
                mesh_tray::runtime_config::Config::Existing(path) => {
                    writeln!(
                        &log,
                        "Engine config is yours, left alone: {}",
                        path.display()
                    )
                }
            }
            .map_err(|e| e.to_string())?;
        }
        // Their `[[models]]` wins: a `--model` flag would beat the file, so when
        // the file names models the tray passes none and stays out of the way.
        let configured_models = mesh_tray::runtime_config::config_declares_models(&profile);
        let model = if !self.effective_share() {
            None
        } else if configured_models {
            use std::io::Write;
            writeln!(
                &log,
                "Tray model: none, {} declares its own models",
                profile.join("config.toml").display()
            )
            .map_err(|e| e.to_string())?;
            None
        } else if let Some(chosen) = self.app.chosen_model() {
            Some(chosen.to_string())
        } else {
            mesh_tray::model_selection::local_model(&self.settings.connection)?
        };
        self.spawned_model = model.clone();
        if let Some(model) = model.as_deref() {
            use std::io::Write;
            writeln!(&log, "Tray selected model: {model}").map_err(|e| e.to_string())?;
        }
        // A private automatic no-fit must be client-only, not an empty serve
        // list that can fall back to engine model selection. Explicit config wins.
        let serve = self.effective_share()
            && (configured_models
                || model.is_some()
                || matches!(self.settings.connection, settings::Connection::Automatic));
        Engine::start(lifecycle::EngineSpec {
            settings: self.settings.clone(),
            profile,
            model,
            share_compute: serve,
            isolated_network: false,
        })
    }

    fn open(&mut self, path: &'static str) {
        if self.child.is_none() || self.error.is_some() {
            let log = self.root.join("mesh.log");
            let message = self.error.as_deref().unwrap_or("Mesh is not running");
            eprintln!("{message}");
            // Native text viewer, not a second settings application. Works even
            // when startup failed before the management server could exist.
            let details = self.root.join("STARTUP_ERROR.txt");
            // The runtime's own last words, so the reason is in front of the
            // user instead of in a log they have to go and read.
            let reason = match mesh_tray::startup_log::reason(&log, self.log_mark) {
                Some(reason) => match mesh_tray::startup_log::advice(&reason) {
                    Some(advice) => {
                        format!("Mesh said:\n\n{reason}\n\nWhat that means:\n\n{advice}\n\n")
                    }
                    None => format!("Mesh said:\n\n{reason}\n\n"),
                },
                None => String::new(),
            };
            let body = format!(
                "Mesh needs attention\n\n{message}\n\n{reason}Quit and reopen Mesh after fixing the installation. Reset tray settings restores launcher defaults, not the engine installation.\nRuntime log: {}\n",
                log.display()
            );
            if std::fs::write(&details, body).is_ok() {
                let _ = open::that(details);
            }
            return;
        }
        if self.snapshot.running {
            if let Err(e) = open::that(format!(
                "http://127.0.0.1:{}{path}",
                self.settings.console_port
            )) {
                self.error = Some(format!("Could not open browser: {e}"));
            }
        } else {
            self.open_when_ready = Some(path);
        }
    }

    fn quit(&mut self) {
        self.restart_at = None;
        self.resetting = false;
        self.pending_settings = None;
        self.retire_private = false;
        if self.stopping.is_some() {
            return;
        }
        let Some(child) = &mut self.child else {
            self.exit = true;
            return;
        };
        match lifecycle::request_stop(child) {
            Ok(()) => self.stopping = Some(Instant::now()),
            Err(e) => self.error = Some(e),
        }
    }

    /// One line for the top of the menu: what the engine is doing now.
    fn status_line(&self) -> String {
        if let Some(error) = &self.error {
            return error
                .lines()
                .next()
                .unwrap_or("Mesh needs attention")
                .to_string();
        }
        if self.stopping.is_some() || self.pending_settings.is_some() {
            return "Mesh is restarting…".into();
        }
        if self.child.is_none() || !self.snapshot.running {
            // A first launch can sit on the macOS keychain prompt; say so
            // instead of failing, since killing the engine re-prompts.
            if self
                .started
                .is_some_and(|at| at.elapsed() > Duration::from_secs(15))
            {
                return "Starting… allow keychain access if asked".into();
            }
            return "Mesh is starting…".into();
        }
        // Engine ids can be content hashes; show the ladder name instead.
        let mut text = status::describe(&self.snapshot);
        if let Some(label) = self
            .spawned_model
            .as_deref()
            .map(mesh_tray::app_config::label)
        {
            for id in &self.snapshot.serving_models {
                let short = id.rsplit('/').next().unwrap_or(id);
                text = text.replace(id.as_str(), label).replace(short, label);
            }
        }
        text
    }

    fn pay_target(&self) -> Option<(u16, u32)> {
        let child = self.child.as_ref()?;
        (self.snapshot.running
            && !self.resetting
            && self.stopping.is_none()
            && self.pending_settings.is_none())
        .then(|| (self.settings.console_port, child.id()))
    }

    /// One action, from the tray menu or the panel.
    fn dispatch(&mut self, id: &str) {
        if self.resetting && id != "quit" {
            return;
        }
        let target = self.pay_target();
        let models = self.snapshot.serving_models.clone();
        if id.starts_with("pay:") && !settings::payments_visible(&self.settings.connection) {
            return;
        }
        if self.pay.click(id, target, &models) {
            return;
        }
        if id.starts_with("share:") {
            self.change_share_rules(id);
            return;
        }
        match id {
            "chat" => self.open("/chat"),
            "panel" => {
                if !open_panel() {
                    self.raise += 1;
                    self.publish_state();
                }
            }
            "compute" => self.toggle_compute(),
            "public" => self.change_mode(settings::Connection::Automatic),
            "private" => self.change_mode(settings::Connection::Private { invite: None }),
            "invite" => self.invite(),
            "join" => self.join(),
            "reset" => self.reset_settings(),
            "quit" => self.quit(),
            _ => {}
        }
    }

    /// What the panel shows; written only when it changes.
    fn publish_state(&mut self) {
        let (sharing, busy) = self.compute_view();
        let model = self
            .spawned_model
            .as_deref()
            .filter(|_| self.snapshot.running && !self.snapshot.serving_models.is_empty())
            .map(|m| mesh_tray::app_config::label(m).to_string());
        let state = mesh_tray::panel_ipc::State {
            status: self.status_line(),
            sharing,
            busy,
            share_status: self.share.status_text(&self.settings.share_rules, sharing),
            serving: model,
            public: settings::payments_visible(&self.settings.connection),
            balance_sats: self.pay.balance().map(|m| m / 1000),
            wallet: self.pay.has_wallet(),
            tray_pid: std::process::id(),
            raise: self.raise,
        };
        if self.published.as_ref() != Some(&state) {
            // Cache only on success, so a failed write retries next tick.
            if mesh_tray::panel_ipc::write_state(&self.root, &state).is_ok() {
                self.published = Some(state);
            }
        }
    }

    fn tick(&mut self) {
        let mut ids: Vec<String> = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            ids.push(event.id.as_ref().to_string());
        }
        ids.extend(mesh_tray::panel_ipc::take_commands(&self.root));
        for id in ids {
            self.dispatch(&id);
        }
        while let Ok(snapshot) = self.rx.try_recv() {
            self.polling = false;
            // A listener is not permission to adopt or display an unrelated instance.
            self.snapshot = if self
                .child
                .as_ref()
                .is_some_and(|child| snapshot.pid == Some(child.id()))
            {
                snapshot
            } else {
                status::Snapshot::default()
            };
            if self.snapshot.running {
                self.started = None;
                if let Some(path) = self.open_when_ready.take() {
                    self.open(path);
                }
            }
        }
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(code)) => {
                    self.child = None;
                    // Clear this child's deadline before apply_pending can start a new one.
                    self.started = None;
                    self.snapshot = status::Snapshot::default();
                    if self.stopping.take().is_some() {
                        if code != "stopped" {
                            self.pending_settings = None;
                            self.retire_private = false;
                            self.error = Some(format!("Shutdown was not verified: {code}. Quit and reopen Mesh. Reset was not completed."));
                        } else if self.pending_settings.is_some() {
                            self.apply_pending();
                        } else {
                            self.exit = true;
                        }
                    } else {
                        if !self.restart_used && !self.resetting {
                            self.restart_used = true;
                            self.restart_at = Some(Instant::now() + POLL);
                            self.error = Some("Mesh stopped, restarting…".into());
                        } else {
                            self.error = Some(format!(
                                "Mesh exited ({code}). Choose Open Chat to see startup details, then quit and reopen Mesh."
                            ));
                        }
                    }
                }
                Err(e) => self.error = Some(format!("Cannot check Mesh process: {e}")),
                Ok(None) => {}
            }
        }
        if self.restart_at.is_some_and(|at| Instant::now() >= at) && !self.exit {
            self.restart_at = None;
            // Never bypass the SDK's unproven-exit guard to start a second runtime.
            match Engine::verify_restart_safe() {
                Ok(()) => self.start(),
                Err(e) => self.error = Some(e),
            }
        }
        if self
            .stopping
            .is_some_and(|at| at.elapsed() > Duration::from_secs(20))
        {
            self.error = Some(
                "Mesh is still stopping. App remains open and will finish the requested change when shutdown completes. No other processes were stopped."
                    .into(),
            );
        }
        // Changing items while the user has the menu open makes macOS close
        // it; cosmetic updates wait until it is closed.
        let menu_open = native::menu_open();
        let rules = self.settings.share_rules.clone();
        self.share
            .tick(&rules, self.settings.share_compute, !menu_open);
        self.follow_share_rules();
        if !menu_open {
            let text = self.status_line();
            if let Some(ui) = self
                .ui
                .as_ref()
                .filter(|ui| ui.status.text() != menu_line(&text, self.error.is_some()))
            {
                ui.status.set_text(menu_line(&text, self.error.is_some()));
                let _ = ui._tray.set_tooltip(Some(format!("Mesh · {text}")));
            }
        }
        let target = self.pay_target();
        let public = settings::payments_visible(&self.settings.connection);
        self.pay.tick(target, public);
        if self.pay.take_restart_request() {
            // Reload the engine so it discovers the newly installed wallet.
            self.queue_settings(self.settings.clone());
        }
        if let Some(id) = self.pay.take_replay(self.pay_target()).filter(|_| public) {
            let models = self.snapshot.serving_models.clone();
            self.pay.click(&id, self.pay_target(), &models);
        }
        if let Some(result) = self.pay.take_reset_result() {
            if self.resetting {
                self.complete_reset(result);
            }
        }
        self.apply_price();
        self.publish_state();
        if !self.polling && Instant::now() >= self.next_poll {
            self.polling = self.tx.send(self.settings.console_port).is_ok();
            self.next_poll = Instant::now() + POLL;
        }
    }
    /// Requested sharing state, and whether an engine restart is in flight.
    fn compute_view(&self) -> (bool, bool) {
        let busy = self.resetting
            || self.stopping.is_some()
            || self.pending_settings.is_some()
            || self.started.is_some();
        let sharing = self
            .pending_settings
            .as_ref()
            .map_or(self.settings.share_compute, |next| next.share_compute);
        (sharing, busy)
    }

    /// Push the panel's price for the served model once per engine run.
    /// Mesh owns pricing; this only replays the saved preference.
    fn apply_price(&mut self) {
        if !settings::payments_visible(&self.settings.connection) {
            return;
        }
        let (Some((port, pid)), Some(model)) = (
            self.pay_target(),
            self.snapshot.serving_models.first().cloned(),
        ) else {
            return;
        };
        let price = self
            .spawned_model
            .as_deref()
            .and_then(|m| self.app.price_for(m));
        let want = Some((pid, model.clone(), price));
        if self.applied_price == want {
            return;
        }
        self.applied_price = want;
        std::thread::spawn(move || {
            use mesh_tray::payments::{Client, Command, Pricing};
            let value = price.map(|p| Pricing {
                input_msat_per_million: p.input_sats * 1000,
                output_msat_per_million: p.output_sats * 1000,
                cache_discount_percent: 0,
            });
            let r = Client::new(port, pid).execute::<serde_json::Value>(&Command::SetPricing {
                model: model.clone(),
                value,
            });
            eprintln!("Panel price for {model}: {price:?} -> {r:?}");
        });
    }

    /// The user's choice, unless a power/sunshine rule pauses it.
    fn effective_share(&self) -> bool {
        self.settings.share_compute && self.share.allows()
    }

    /// Restart the engine when a rule flips whether it should serve.
    fn follow_share_rules(&mut self) {
        let idle = self.child.is_some()
            && self.started.is_none()
            && self.stopping.is_none()
            && self.pending_settings.is_none()
            && !self.resetting;
        let next = mesh_tray::app_config::AppConfig::load(&self.root);
        if next != self.app {
            self.app = next;
        }
        let model_changed = self.spawned_share
            && self.app.chosen_model().is_some()
            && self.app.chosen_model() != self.spawned_model.as_deref();
        if idle && (self.effective_share() != self.spawned_share || model_changed) {
            self.queue_settings(self.settings.clone());
        }
    }

    /// Rules are launcher state only; saving them never restarts by itself.
    fn change_share_rules(&mut self, id: &str) {
        let current = self.settings.share_rules.clone();
        if let Some(rules) = self.share.click(id, &current) {
            let mut next = self.settings.clone();
            next.share_rules = rules.clone();
            match next.save(&self.root) {
                Ok(()) => {
                    self.settings = next;
                    if let Some(p) = self.pending_settings.as_mut() {
                        p.share_rules = rules;
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }
        let rules = self.settings.share_rules.clone();
        self.share.reset(&rules);
    }

    fn toggle_compute(&mut self) {
        // muda auto-toggles the check before dispatch; queue first, then
        // render the requested state with its transition label.
        let busy = self.resetting
            || self.stopping.is_some()
            || self.pending_settings.is_some()
            || self.started.is_some();
        if !busy {
            let mut next = self.settings.clone();
            next.share_compute = !next.share_compute;
            self.queue_settings(next);
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod desktop {
    use super::*;
    use winit::application::ApplicationHandler;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.ui.is_none() {
                if let Err(e) = self.build() {
                    eprintln!("Cannot create Mesh tray: {e}");
                    event_loop.exit();
                }
            }
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: winit::event::WindowEvent,
        ) {
        }
        fn new_events(&mut self, event_loop: &ActiveEventLoop, _: winit::event::StartCause) {
            if self.ui.is_none() {
                return;
            }
            self.tick();
            if self.exit {
                event_loop.exit();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(100),
            ));
        }
    }
    pub fn run(mut app: App) -> Result<(), String> {
        let mut builder = EventLoop::builder();
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }
        let event_loop = builder.build().map_err(|e| e.to_string())?;
        event_loop.run_app(&mut app).map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "linux")]
mod desktop {
    use super::*;
    use gtk::prelude::*;
    fn tray_host_available() -> bool {
        let Ok(connection) = zbus::blocking::Connection::session() else {
            return false;
        };
        let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&connection) else {
            return false;
        };
        let Ok(name) = zbus::names::BusName::try_from("org.kde.StatusNotifierWatcher") else {
            return false;
        };
        proxy.name_has_owner(name).unwrap_or(false)
    }
    pub fn run(mut app: App) -> Result<(), String> {
        gtk::init().map_err(|e| e.to_string())?;
        let fallback = !tray_host_available();
        app.build()?;
        let app = std::rc::Rc::new(std::cell::RefCell::new(app));
        let fallback_status = gtk::Label::new(Some("Mesh · Getting ready…"));
        // GNOME without an extension must never leave an invisible daemon.
        let window = if fallback {
            let window = gtk::Window::new(gtk::WindowType::Toplevel);
            window.set_title("Mesh");
            window.set_default_size(320, 180);
            let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
            content.set_border_width(24);
            content.add(&fallback_status);
            for (label, route) in [("Open Chat", Some("/chat")), ("Quit Mesh", None)] {
                let button = gtk::Button::with_label(label);
                let app = app.clone();
                button.connect_clicked(move |_| {
                    let Ok(mut app) = app.try_borrow_mut() else {
                        return;
                    };
                    if let Some(path) = route {
                        app.open(path);
                    } else {
                        app.quit();
                    }
                });
                content.add(&button);
            }
            for (label, action) in [
                ("Public", "public"),
                ("Private", "private"),
                ("Invite someone to your mesh", "invite"),
                ("Join with an invite", "join"),
                ("Reset settings…", "reset"),
            ] {
                let button = gtk::Button::with_label(label);
                let app = app.clone();
                button.connect_clicked(move |_| {
                    let Ok(mut app) = app.try_borrow_mut() else {
                        return;
                    };
                    match action {
                        "public" => app.change_mode(settings::Connection::Automatic),
                        "private" => {
                            app.change_mode(settings::Connection::Private { invite: None })
                        }
                        "invite" => app.invite(),
                        "join" => app.join(),
                        "reset" => app.reset_settings(),
                        _ => {}
                    }
                });
                content.add(&button);
            }
            window.add(&content);
            let closing = app.clone();
            window.connect_delete_event(move |_, _| {
                if let Ok(mut app) = closing.try_borrow_mut() {
                    app.quit();
                }
                gtk::glib::Propagation::Stop
            });
            window.show_all();
            Some(window)
        } else {
            None
        };
        gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
            let _ = &window;
            let Ok(mut app) = app.try_borrow_mut() else {
                return gtk::glib::ControlFlow::Continue;
            };
            app.tick();
            if fallback {
                let text = if let Some(error) = &app.error {
                    format!("Mesh needs attention: {error}")
                } else if app.stopping.is_some() {
                    "Mesh · Applying change…".into()
                } else if app.snapshot.running {
                    "Mesh · Running".into()
                } else {
                    "Mesh · Getting ready…".into()
                };
                fallback_status.set_text(&text);
                fallback_status.set_line_wrap(true);
            }
            if app.exit {
                gtk::main_quit();
                gtk::glib::ControlFlow::Break
            } else {
                gtk::glib::ControlFlow::Continue
            }
        });
        gtk::main();
        Ok(())
    }
}

/// Redirect before creating any threads. Native and Rust output share the
/// existing log; no second credential unlock or process is involved.
#[cfg(unix)]
fn capture_logs(root: &std::path::Path) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(root.join("mesh.log"))
        .map_err(|e| e.to_string())?;
    for fd in [libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        if unsafe { libc::dup2(log.as_raw_fd(), fd) } == -1 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn capture_logs(_: &std::path::Path) -> Result<(), String> {
    Ok(())
}

/// One short menu-bar line; detail lives in the panel and tooltip.
fn menu_line(status: &str, error: bool) -> String {
    if error {
        return "Needs attention · open Settings".into();
    }
    let s = status.trim_end_matches('…');
    if s.chars().count() <= 28 {
        status.into()
    } else {
        format!("{}…", s.chars().take(27).collect::<String>())
    }
}

fn main() {
    let result = (|| {
        // The engine invokes its built-in blobstore through current_exe().
        // Dispatch before profile locking or UI setup; this is not another node.
        let args: Vec<String> = std::env::args().skip(1).collect();
        // Wallet providers are installed executables, never tray re-execs.
        if let ["--log-format", "json", "--plugin", name @ "blobstore"] = args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            return tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?
                .block_on(mesh_llm_sdk::plugin::run_plugin_process(
                    (*name).into(),
                    Vec::new(),
                ))
                .map_err(|e| format!("{e:#}"));
        }
        // The engine runs as a child of the tray; see lifecycle.rs.
        if args == [lifecycle::ENGINE_ARG] {
            return lifecycle::run_engine_process();
        }
        if !args.is_empty() {
            return Err("Unsupported Mesh app arguments".into());
        }
        let root = settings::data_root()?;
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let _profile_lock = identity::lock_profile(&root)?;
        // Process environment is configured before App creates any threads.
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_dir = exe.parent().ok_or("Cannot locate app executable")?;
        // macOS: inside the .app bundle. Windows: beside mesh-tray.exe.
        let runtimes = if cfg!(windows) {
            exe_dir.join("native-runtimes")
        } else {
            exe_dir.join("../Resources/engine/native-runtimes")
        };
        if runtimes.is_dir() {
            std::env::set_var("MESH_LLM_NATIVE_RUNTIME_BUNDLE_DIR", runtimes);
        }
        capture_logs(&root)?;
        let settings = settings::Settings::load(&root)?;
        desktop::run(App::new(root, settings))
    })();
    if let Err(error) = result {
        eprintln!("Mesh: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod transaction_tests {
    #[cfg(target_os = "macos")]
    use crate::portable_test as portable;
    #[cfg(target_os = "macos")]
    #[test]
    fn portable_adapter_api_compiles_without_launching_dialogs() {
        // Clipboard access is the one API that cannot be checked here: the
        // portable adapter's implementation is GTK, gated to Linux.
        let _ = portable::notice;
        let _ = portable::confirm;
        // prompt_card is GTK on the platform this adapter ships on, so it is
        // gated out here; notice and confirm are the portable pair.
    }
    use super::*;
    fn app(root: &std::path::Path) -> App {
        App::new(root.into(), settings::Settings::default())
    }
    #[cfg(unix)]
    #[test]
    fn unexpected_exit_schedules_only_one_retry_and_quit_cancels_it() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        let (engine, _stop, done) = Engine::fixture();
        app.child = Some(engine);
        done.send(Ok(())).unwrap();
        app.tick();
        assert!(app.restart_used);
        assert_eq!(app.error.as_deref(), Some("Mesh stopped, restarting…"));
        assert!(app.restart_at.is_some_and(|at| at > Instant::now()));
        app.quit();
        assert!(app.restart_at.is_none());
        assert!(app.exit);

        app.exit = false;
        let (engine, _stop, done) = Engine::fixture();
        app.child = Some(engine);
        done.send(Ok(())).unwrap();
        app.tick();
        assert!(app.restart_at.is_none());
        assert!(app.error.as_deref().unwrap().starts_with("Mesh exited"));
    }

    #[cfg(unix)]
    #[test]
    fn delayed_retry_reports_spawn_failure_without_starting_a_runtime() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        // The occupied port makes spawn fail before profile/credential access.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        app.settings.console_port = listener.local_addr().unwrap().port();
        app.restart_used = true;
        app.restart_at = Some(Instant::now());
        app.tick();
        assert!(app.restart_at.is_none());
        assert!(app.child.is_none());
        assert!(app.error.as_deref().unwrap().contains("already in use"));
        app.tick();
        assert!(app.child.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn compute_toggle_stops_owned_engine_and_preserves_mesh_until_exit() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        app.settings.accept_seed("selected-mesh").unwrap();
        app.settings.save(root.path()).unwrap();
        let (engine, mut stop, _done) = Engine::fixture();
        app.child = Some(engine);
        app.toggle_compute();
        assert_eq!(stop.try_recv(), Ok(()));
        assert!(app.settings.share_compute);
        assert!(settings::Settings::load(root.path()).unwrap().share_compute);
        let pending = app.pending_settings.as_ref().unwrap();
        assert!(!pending.share_compute);
        assert_eq!(pending.connection, app.settings.connection);
        app.toggle_compute();
        assert!(!app.pending_settings.as_ref().unwrap().share_compute);
    }

    #[test]
    fn failed_save_leaves_current_memory_and_disk_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        std::fs::create_dir(root.path().join("launcher.json")).unwrap();
        let mut candidate = app.settings.clone();
        candidate.accept_seed("a-pasted-invite").unwrap();
        app.pending_settings = Some(candidate);
        app.apply_pending();
        assert!(app.settings.joins().is_empty());
        assert!(app.child.is_none());
        assert!(app.error.as_ref().unwrap().contains("save"));
        assert!(app.pending_settings.is_none());
    }
    #[test]
    fn startup_failure_keeps_the_committed_choice_and_never_rejoins_the_old_mesh() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        app.settings.accept_seed("their-invite").unwrap();
        app.settings.save(root.path()).unwrap();
        // Deterministic occupied-port failure, never start a Mesh process.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        app.settings.console_port = listener.local_addr().unwrap().port();
        app.settings.api_port = if app.settings.console_port == 9447 {
            9448
        } else {
            9447
        };
        // Leaving for Public is committed before the runtime is asked to
        // start; a failed start must not put the old Mesh back.
        let next = mesh_tray::reset::switching_to(&app.settings, settings::Connection::Automatic);
        app.pending_settings = Some(next);
        app.apply_pending();
        assert!(app.child.is_none());
        assert!(app.error.is_some());
        assert!(app.settings.joins().is_empty());
        assert!(settings::Settings::load(root.path())
            .unwrap()
            .joins()
            .is_empty());
        app.start();
        assert!(app.settings.joins().is_empty());
    }
    #[cfg(unix)]
    fn exited_child() -> Engine {
        let (engine, _, finished) = Engine::fixture();
        finished.send(Ok(())).unwrap();
        engine
    }

    #[cfg(unix)]
    #[test]
    fn child_exit_clears_startup_deadline_and_preserves_the_exit_error() {
        for startup_age in [Duration::ZERO, Duration::from_secs(181)] {
            let root = tempfile::tempdir().unwrap();
            let mut app = app(root.path());
            app.polling = true; // Exercise tick without issuing status requests.
            app.restart_used = true; // Exhausted retries retain the exit details.
            app.child = Some(exited_child());
            app.started = Some(Instant::now() - startup_age);
            app.snapshot.running = true;

            app.tick();

            assert!(app.child.is_none());
            assert!(app.started.is_none());
            assert!(!app.snapshot.running);
            let error = app.error.clone().unwrap();
            assert!(error.starts_with("Mesh exited ("));
            assert!(error.contains("quit and reopen Mesh"));
            app.tick();
            assert_eq!(app.error.as_deref(), Some(error.as_str()));
        }
    }

    #[cfg(unix)]
    #[test]
    fn expected_exit_does_not_report_an_expired_startup_deadline() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        app.polling = true;
        app.child = Some(exited_child());
        app.started = Some(Instant::now() - Duration::from_secs(181));
        app.stopping = Some(Instant::now());

        app.tick();

        assert!(app.child.is_none());
        assert!(app.started.is_none());
        assert!(app.stopping.is_none());
        assert!(app.exit);
        assert!(app.error.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn failed_save_after_exit_is_not_overwritten_by_the_old_startup_deadline() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        app.polling = true;
        app.child = Some(exited_child());
        app.started = Some(Instant::now() - Duration::from_secs(181));
        app.stopping = Some(Instant::now());
        app.pending_settings = Some(app.settings.clone());
        // Fail before start() so this regression test cannot launch a Mesh engine.
        std::fs::create_dir(root.path().join("launcher.json")).unwrap();

        app.tick();

        assert!(app.child.is_none());
        assert!(app.started.is_none());
        assert!(app.stopping.is_none());
        assert!(app.pending_settings.is_none());
        assert!(!app.exit);
        assert!(app
            .error
            .as_deref()
            .unwrap()
            .starts_with("Could not save connection or finish reset:"));
    }

    #[cfg(unix)]
    #[test]
    fn slow_start_waits_for_keychain_without_stopping_or_erroring() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        app.polling = true;
        let (engine, _requested, _finished) = Engine::fixture();
        let pid = engine.id();
        app.child = Some(engine);
        app.started = Some(Instant::now());

        app.tick();
        let fresh_start_pending = app.started.is_some() && app.error.is_none();
        app.started = Some(Instant::now() - Duration::from_secs(181));
        app.tick();
        let retained_pid = app.child.as_ref().map(Engine::id);
        // Completion is controlled by the test, not an OS child.
        let mut child = app.child.take().unwrap();
        let still_alive = child.try_wait().unwrap().is_none();

        assert!(fresh_start_pending);
        assert_eq!(retained_pid, Some(pid));
        assert!(still_alive);
        assert!(app.error.is_none());
        assert!(app.status_line().contains("keychain"));
    }

    #[cfg(unix)]
    #[test]
    fn a_choice_is_not_saved_until_the_owned_child_is_reaped_and_busy_actions_do_not_replace_it() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        app.settings.save(root.path()).unwrap();
        let (mut other, mut other_stop, _other_done) = Engine::fixture();
        let (engine, mut requested, finished) = Engine::fixture();
        app.child = Some(engine);
        let mut next = app.settings.clone();
        next.accept_seed("their-invite").unwrap();
        app.queue_settings(next);
        assert!(settings::Settings::load(root.path())
            .unwrap()
            .joins()
            .is_empty());
        app.queue_settings(settings::Settings::default());
        assert_eq!(app.pending_settings.as_ref().unwrap().joins().len(), 1);
        assert_eq!(requested.try_recv(), Ok(()));
        assert!(app.child.as_mut().unwrap().try_wait().unwrap().is_none());
        finished.send(Ok(())).unwrap();
        assert!(app.child.as_mut().unwrap().try_wait().unwrap().is_some());
        app.child = None;
        app.stopping = None;
        // Force save failure so this test cannot spawn a runtime after reaping.
        std::fs::remove_file(root.path().join("launcher.json")).unwrap();
        std::fs::create_dir(root.path().join("launcher.json")).unwrap();
        app.apply_pending();
        assert!(app.settings.joins().is_empty());
        assert!(app.child.is_none());
        let alive = other.try_wait().unwrap().is_none();
        assert!(other_stop.try_recv().is_err());
        assert!(alive);
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "native_portable.rs"]
mod portable_test;

/// Open the GPUI panel (sibling `mesh-panel` binary). Returns false when one
/// is already open; the caller then asks it to come forward.
fn open_panel() -> bool {
    static PANEL: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);
    let mut panel = PANEL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(child) = panel.as_mut() {
        if matches!(child.try_wait(), Ok(None)) {
            return false;
        }
    }
    let Ok(exe) = std::env::current_exe() else {
        return true;
    };
    let path = exe.with_file_name("mesh-panel");
    match std::process::Command::new(&path).spawn() {
        Ok(child) => *panel = Some(child),
        Err(e) => native::notice(
            "Could not open the Mesh panel",
            &format!("{}: {e}", path.display()),
        ),
    }
    true
}

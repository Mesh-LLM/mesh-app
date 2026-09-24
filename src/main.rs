#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use mesh_tray::{identity, settings};
mod compute_menu;
mod invites;
mod lifecycle;
#[cfg(target_os = "macos")]
mod native;
#[cfg(not(target_os = "macos"))]
#[path = "native_portable.rs"]
mod native;
mod pay_menu;
mod qr;
mod status;

use lifecycle::Engine;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const POLL: Duration = Duration::from_secs(3);

struct Ui {
    compute: compute_menu::ComputeMenu,
    public: muda::CheckMenuItem,
    private: muda::CheckMenuItem,
    _tray: TrayIcon,
}

struct App {
    settings: settings::Settings,
    pending_settings: Option<settings::Settings>,
    root: PathBuf,
    ui: Option<Ui>,
    child: Option<Engine>,
    rx: Receiver<status::Snapshot>,
    tx: Sender<()>,
    snapshot: status::Snapshot,
    next_poll: Instant,
    polling: bool,
    started: Option<Instant>,
    stopping: Option<Instant>,
    error: Option<String>,
    /// Log length when the current run was started, so a fatal line from an
    /// earlier run is never quoted as this run's reason.
    log_mark: u64,
    open_when_ready: Option<&'static str>,
    exit: bool,
    pay: pay_menu::Payments,
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
        let port = settings.console_port;
        let (tx, jobs) = mpsc::channel();
        let (results, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while jobs.recv().is_ok() {
                if results.send(status::snapshot(port)).is_err() {
                    break;
                }
            }
        });
        Self {
            root,
            settings,
            pending_settings: None,
            ui: None,
            child: None,
            rx,
            tx,
            snapshot: status::Snapshot::default(),
            next_poll: Instant::now(),
            polling: false,
            started: None,
            stopping: None,
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
        let quit = MenuItem::with_id("quit", "Quit Mesh", true, None);
        let private_mode = matches!(
            self.settings.connection,
            settings::Connection::Private { .. }
        );
        let public = muda::CheckMenuItem::with_id("public", "Public", true, !private_mode, None);
        let private = muda::CheckMenuItem::with_id("private", "Private", true, private_mode, None);
        let people = muda::Submenu::new("Invites", true);
        people
            .append_items(&[
                &MenuItem::with_id("invite", "Invite someone…", true, None),
                &MenuItem::with_id("join", "Join with an invite…", true, None),
            ])
            .map_err(|e| e.to_string())?;
        let retry = MenuItem::with_id("retry", "Retry startup…", true, None);
        let payments = self.pay.submenu()?;
        let compute = compute_menu::ComputeMenu::new(self.settings.share_compute);
        menu.append_items(&[
            &chat,
            compute.item(),
            &PredefinedMenuItem::separator(),
            &public,
            &private,
            &people,
            &payments,
            &PredefinedMenuItem::separator(),
            &retry,
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
            compute,
            public,
            private,
            _tray: tray,
        });
        self.start();
        Ok(())
    }

    fn start(&mut self) {
        if self.child.is_some() {
            return;
        }
        self.error = None;
        self.log_mark = std::fs::metadata(self.root.join("mesh.log"))
            .map(|m| m.len())
            .unwrap_or(0);
        match self.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.started = Some(Instant::now());
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn spawn(&self) -> Result<Engine, String> {
        if cfg!(windows) {
            return Err("Windows is not supported by this tray yet. Your existing Mesh state was not touched.".into());
        }
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
        let model = if !self.settings.share_compute {
            None
        } else if mesh_tray::runtime_config::config_declares_models(&profile) {
            use std::io::Write;
            writeln!(
                &log,
                "Tray model: none, {} declares its own models",
                profile.join("config.toml").display()
            )
            .map_err(|e| e.to_string())?;
            None
        } else {
            mesh_tray::model_selection::local_model(&self.settings.connection)?
        };
        if let Some(model) = model.as_deref() {
            use std::io::Write;
            writeln!(&log, "Tray selected private model: {model}").map_err(|e| e.to_string())?;
        }
        let config = lifecycle::config(&self.settings, &profile, model);
        Engine::start(config, self.settings.share_compute)
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
                "Mesh needs attention\n\n{message}\n\n{reason}Use Retry startup after fixing the installation.\nRuntime log: {}\n",
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
        if self.stopping.is_some() {
            return;
        }
        self.pending_settings = None;
        let Some(child) = &mut self.child else {
            self.exit = true;
            return;
        };
        match lifecycle::request_stop(child) {
            Ok(()) => self.stopping = Some(Instant::now()),
            Err(e) => self.error = Some(e),
        }
    }

    /// Our own runtime's management port and pid, only once it is ready.
    fn pay_target(&self) -> Option<(u16, u32)> {
        let child = self.child.as_ref()?;
        (self.snapshot.running && self.stopping.is_none() && self.pending_settings.is_none())
            .then(|| (self.settings.console_port, child.id()))
    }

    fn tick(&mut self) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let target = self.pay_target();
            let models = self.snapshot.serving_models.clone();
            if self.pay.click(event.id.as_ref(), target, &models) {
                continue;
            }
            match event.id.as_ref() {
                "chat" => self.open("/chat"),
                "compute" => self.toggle_compute(),
                "public" => self.change_mode(settings::Connection::Automatic),
                "private" => self.change_mode(settings::Connection::Private { invite: None }),
                "invite" => self.invite(),
                "join" => self.join(),
                "retry" => self.start(),
                "quit" => self.quit(),
                _ => {}
            }
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
                        if self.pending_settings.is_some() {
                            self.apply_pending();
                        } else {
                            self.exit = true;
                        }
                    } else {
                        self.error = Some(format!(
                            "Mesh exited ({code}). Choose Open Chat to see startup details, then Retry startup."
                        ));
                    }
                }
                Err(e) => self.error = Some(format!("Cannot check Mesh process: {e}")),
                Ok(None) => {}
            }
        }
        if self
            .stopping
            .is_some_and(|at| at.elapsed() > Duration::from_secs(20))
        {
            self.stopping = None;
            self.pending_settings = None;
            self.error = Some(
                "Mesh did not stop. App remains open; retry Quit. No other processes were stopped."
                    .into(),
            );
        }
        if self
            .started
            .is_some_and(|at| at.elapsed() > Duration::from_secs(180))
        {
            self.started = None;
            self.error = Some(
                "Mesh startup timed out. Choose Open Chat for startup details; Quit stops only this instance."
                    .into(),
            );
        }
        let (sharing, busy) = self.compute_view();
        if let Some(ui) = &self.ui {
            ui.compute.sync(sharing, busy);
        }
        let target = self.pay_target();
        self.pay.tick(target);
        if !self.polling && Instant::now() >= self.next_poll {
            self.polling = self.tx.send(()).is_ok();
            self.next_poll = Instant::now() + POLL;
        }
    }
    // Only called for a mode click or a successfully committed settings change.
    // Never refresh menu state from the periodic polling/render path.
    fn sync_mode_checks(&self) {
        let Some(ui) = &self.ui else { return };
        let private = matches!(
            self.settings.connection,
            settings::Connection::Private { .. }
        );
        if ui.public.is_checked() == private {
            ui.public.set_checked(!private);
        }
        if ui.private.is_checked() != private {
            ui.private.set_checked(private);
        }
    }

    /// Requested sharing state, and whether an engine restart is in flight.
    fn compute_view(&self) -> (bool, bool) {
        let busy =
            self.stopping.is_some() || self.pending_settings.is_some() || self.started.is_some();
        let sharing = self
            .pending_settings
            .as_ref()
            .map_or(self.settings.share_compute, |next| next.share_compute);
        (sharing, busy)
    }

    fn toggle_compute(&mut self) {
        // muda auto-toggles the check before dispatch; queue first, then
        // render the requested state with its transition label.
        let busy =
            self.stopping.is_some() || self.pending_settings.is_some() || self.started.is_some();
        if !busy {
            let mut next = self.settings.clone();
            next.share_compute = !next.share_compute;
            self.queue_settings(next);
        }
        let (sharing, busy) = self.compute_view();
        if let Some(ui) = &self.ui {
            ui.compute.sync(sharing, busy);
        }
    }

    fn change_mode(&mut self, connection: settings::Connection) {
        // muda auto-toggles the clicked item before dispatch. Restore the saved
        // choice so cancellation, same-mode clicks and failures cannot lie.
        self.sync_mode_checks();
        if self.stopping.is_some()
            || self.pending_settings.is_some()
            || std::mem::discriminant(&self.settings.connection)
                == std::mem::discriminant(&connection)
        {
            return;
        }
        let leaving_joined_mesh = matches!(
            self.settings.connection,
            settings::Connection::Private { .. }
        ) && !self.settings.joins().is_empty();
        let (title, body) = if leaving_joined_mesh {
            (
                "Leave this private Mesh?",
                "Going Public forgets the invite that put you in it, and restarts Mesh. It does not remove you for anyone else: rejoining means pasting an invite again. Your identity, your models and your settings stay as they are.",
            )
        } else if matches!(connection, settings::Connection::Private { .. }) {
            (
                "Start a private Mesh?",
                "Restarts Mesh as its own private Mesh, with nobody in it yet — copy an invite and send it to the people you want. Anyone who has it can join and pass it on.",
            )
        } else {
            (
                "Share with anyone?",
                "Restarts Mesh in Public, where it serves whoever finds it. Anything outstanding is cancelled.",
            )
        };
        if !native::confirm(title, body, "Change connection") {
            return;
        }
        if matches!(connection, settings::Connection::Private { .. }) {
            let profile = match settings::mesh_profile() {
                Ok(profile) => profile,
                Err(e) => {
                    native::notice("Could not set up Private", &e);
                    return;
                }
            };
            if let Err(e) = identity::establish(&profile) {
                native::notice("Could not set up Private", &e);
                return;
            }
        }
        // Switching Mesh *is* forgetting this one: the people, the outstanding
        // invitations all belong to the Mesh being left, so there
        // is no separate "start over" to find.
        let next = mesh_tray::reset::switching_to(&self.settings, connection);
        self.queue_settings(next);
    }

    fn queue_settings(&mut self, next: settings::Settings) {
        if self.stopping.is_some() || self.pending_settings.is_some() {
            return;
        }
        self.pending_settings = Some(next);
        self.open_when_ready = None;
        if let Some(child) = &mut self.child {
            match lifecycle::request_stop(child) {
                Ok(()) => self.stopping = Some(Instant::now()),
                Err(e) => {
                    self.error = Some(e);
                    self.pending_settings = None;
                }
            }
        } else {
            self.apply_pending();
        }
    }

    fn apply_pending(&mut self) {
        if let Some(next) = self.pending_settings.take() {
            match next.save(&self.root) {
                Ok(()) => {
                    self.settings = next;
                    self.sync_mode_checks();
                    self.snapshot = status::Snapshot::default();
                    self.error = None;
                    self.start();
                }
                Err(e) => {
                    self.error = Some(format!("Could not save connection: {e}"));
                }
            }
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
                ("Retry startup", "retry"),
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
                        "retry" => app.start(),
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

fn main() {
    let result = (|| {
        // The engine invokes its built-in blobstore through current_exe().
        // Dispatch before profile locking or UI setup; this is not another node.
        let args: Vec<String> = std::env::args().skip(1).collect();
        // Same for the built-in Lexe wallet (PR #1926): Mesh launches it as
        // `current_exe --plugin wallet-lexe` and owns it; the tray only dispatches.
        if let ["--log-format", "json", "--plugin", name @ ("blobstore" | "wallet-lexe")] = args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            return tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?
                .block_on(mesh_llm_host_runtime::plugin::run_plugin_process(
                    (*name).into(),
                ))
                .map_err(|e| format!("{e:#}"));
        }
        if !args.is_empty() {
            return Err("Unsupported Mesh app arguments".into());
        }
        let root = settings::data_root()?;
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let _profile_lock = identity::lock_profile(&root)?;
        // Process environment is configured before App creates any threads.
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let runtimes = exe
            .parent()
            .ok_or("Cannot locate app executable")?
            .join("../Resources/engine/native-runtimes");
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
            app.child = Some(exited_child());
            app.started = Some(Instant::now() - startup_age);
            app.snapshot.running = true;

            app.tick();

            assert!(app.child.is_none());
            assert!(app.started.is_none());
            assert!(!app.snapshot.running);
            let error = app.error.clone().unwrap();
            assert!(error.starts_with("Mesh exited ("));
            assert!(error.contains("Retry startup"));
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
            .starts_with("Could not save connection:"));
    }

    #[cfg(unix)]
    #[test]
    fn live_child_still_times_out_without_being_stopped_or_forgotten() {
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
        assert!(app.started.is_none());
        let error = app.error.clone().unwrap();
        assert!(error.starts_with("Mesh startup timed out."));
        app.tick();
        assert_eq!(app.error.as_deref(), Some(error.as_str()));
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

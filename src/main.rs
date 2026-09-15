#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use mesh_tray::{admission, consent, identity, settings};
mod lifecycle;
mod membership_ui;
#[cfg(target_os = "macos")]
mod native;
#[cfg(not(target_os = "macos"))]
#[path = "native_portable.rs"]
mod native;
mod sharing;
mod status;

use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const POLL: Duration = Duration::from_secs(3);

struct Ui {
    menu: Menu,
    status: MenuItem,
    public: muda::CheckMenuItem,
    private: muda::CheckMenuItem,
    retry: MenuItem,
    retry_visible: bool,
    quit: MenuItem,
    people: muda::Submenu,
    people_ids: Vec<String>,
    people_offer: Option<Option<&'static str>>,
    _tray: TrayIcon,
}

struct App {
    settings: settings::Settings,
    native: native::Native,
    pending_settings: Option<settings::Settings>,
    root: PathBuf,
    ui: Option<Ui>,
    child: Option<Child>,
    rx: Receiver<status::Snapshot>,
    tx: Sender<()>,
    snapshot: status::Snapshot,
    next_poll: Instant,
    polling: bool,
    started: Option<Instant>,
    stopping: Option<Instant>,
    pending_reset: bool,
    error: Option<String>,
    open_when_ready: Option<&'static str>,
    exit: bool,
    offer_reply: bool,
    /// A card was just produced -- an RSVP, or a confirmation -- and still has
    /// to reach the other person. Offered once the restart that produced it is
    /// finished, so nobody has to find a menu item for it.
    offer_membership_card: bool,
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
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 255));
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
            native: native::Native::default(),
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
            pending_reset: false,
            error: None,
            open_when_ready: None,
            exit: false,
            offer_reply: false,
            offer_membership_card: false,
        }
    }

    fn build(&mut self) -> Result<(), String> {
        let menu = Menu::new();
        let status = MenuItem::new("Mesh · Getting ready…", false, None);
        let chat = MenuItem::with_id("chat", "Open Chat…", true, None);
        let settings = MenuItem::with_id("settings", "Settings…", true, None);
        let quit = MenuItem::with_id("quit", "Quit Mesh", true, None);
        let public = muda::CheckMenuItem::with_id("public", "Public", true, false, None);
        let private = muda::CheckMenuItem::with_id("private", "Private", true, false, None);
        let people = muda::Submenu::new("Members", true);
        let retry = MenuItem::with_id("retry", "Retry startup…", true, None);
        let reset = MenuItem::with_id("reset", "Start Over (Forget This Mesh)…", true, None);
        menu.append_items(&[
            &chat,
            &PredefinedMenuItem::separator(),
            &status,
            &public,
            &private,
            &people,
            &PredefinedMenuItem::separator(),
            &settings,
            &reset,
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
            menu,
            status,
            public,
            private,
            retry,
            retry_visible: false,
            quit,
            people,
            people_ids: Vec::new(),
            people_offer: None,
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
        match self.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.started = Some(Instant::now());
            }
            Err(e) => self.error = Some(e),
        }
        self.render();
    }

    fn spawn(&self) -> Result<Child, String> {
        if cfg!(windows) {
            return Err("Windows runtime launch is unavailable until Mesh supports isolated identity and trust paths. Your existing Mesh state was not touched.".into());
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
        let binary = settings::binary()?;
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        if matches!(self.settings.connection, settings::Connection::Automatic)
            && self.root.join("public/key").exists()
            && !self.root.join("public-home/.mesh-llm/key").exists()
        {
            return Err("This profile has an established development-runtime public identity. It was preserved. Use \"Start Over\" to forget it, or a fresh demo profile, until its migration is reviewed.".into());
        }
        let home = self.settings.runtime_home(&self.root);
        mesh_tray::runtime_home::prepare(&home)?;
        if matches!(
            self.settings.connection,
            settings::Connection::Private { .. }
        ) {
            // Startup verifies the identity without unlocking it; the runtime
            // child unlocks the key, so the user sees one prompt, not two.
            identity::establish(&self.root)?;
            admission::prepare_store(&home, &self.settings.admitted_owners)?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("mesh.log"))
            .map_err(|e| e.to_string())?;
        let model = mesh_tray::model_selection::local_model(&self.settings.connection)?;
        let mut command = Command::new(binary);
        if let Some(model) = model.as_deref() {
            use std::io::Write;
            writeln!(&log, "Tray selected private model: {model}").map_err(|e| e.to_string())?;
        }
        {
            use std::io::Write;
            // Thinking off for tray chat. A config the user has taken over is
            // reported and left alone, never replaced.
            match mesh_tray::runtime_config::ensure_defaults(&home)? {
                Some(path) => writeln!(&log, "Tray engine defaults: {}", path.display()),
                None => writeln!(
                    &log,
                    "Tray engine defaults: skipped, {} is yours",
                    home.join(".mesh-llm/config.toml").display()
                ),
            }
            .map_err(|e| e.to_string())?;
        }
        command
            .args(self.settings.args())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log))
            .env_remove("MESH_LLM_EPHEMERAL_KEY")
            .env_remove("MESH_LLM_OWNER_PASSPHRASE");
        mesh_tray::runtime_home::configure(&mut command, &home);
        if let Some(model) = model {
            command.args(["--model", model.as_str()]);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        command
            .spawn()
            .map_err(|e| format!("Could not start Mesh: {e}"))
    }

    fn restore_mode_checks(&self) {
        let Some(ui) = &self.ui else { return };
        let private = matches!(
            self.settings.connection,
            settings::Connection::Private { .. }
        );
        ui.public.set_checked(!private);
        ui.private.set_checked(private);
    }

    fn render(&mut self) {
        self.restore_mode_checks();
        let Some(ui) = &mut self.ui else { return };
        // Two actions cover the whole journey: invite someone, or accept the
        // card they sent back. Each card is copied the moment it is made, so
        // the third entry is only a retry for a card that was lost before it
        // reached them, and it is offered only when such a card exists.
        // Who is already joined is the console's job, not a menu's.
        let offer = self
            .settings
            .membership_receipt
            .as_deref()
            .and_then(mesh_tray::invitation::card_kind)
            .map(|kind| match kind {
                "confirmation" => "Copy your members card again",
                "acceptance" => "Copy your RSVP again",
                _ => "Copy your last card again",
            });
        if ui.people_ids != self.settings.admitted_owners
            || ui.people_offer != Some(offer)
            || ui.people.items().is_empty()
        {
            while ui.people.remove_at(0).is_some() {}
            let _ = ui.people.append_items(&[
                &MenuItem::with_id("invite-member", "Invite someone…", true, None),
                &MenuItem::with_id("paste-card", "Accept an invitation or RSVP", true, None),
            ]);
            if let Some(label) = offer {
                let _ = ui
                    .people
                    .append(&MenuItem::with_id("share-membership", label, true, None));
            }
            // Removing a person has no other home -- the console lists members
            // but cannot revoke one, and the CLI writes a different store --
            // so the list stays, shown only when there is somebody to remove.
            if !self.settings.admitted_owners.is_empty() {
                let _ = ui.people.append(&PredefinedMenuItem::separator());
            }
            for owner in &self.settings.admitted_owners {
                let name = self
                    .settings
                    .owner_names
                    .get(owner)
                    .map(String::as_str)
                    .unwrap_or("Mesh person");
                let label = format!("Remove {} · {}…", name, &owner[..12]);
                let _ = ui.people.append(&MenuItem::with_id(
                    format!("remove:{owner}"),
                    label,
                    true,
                    None,
                ));
            }
            ui.people_ids = self.settings.admitted_owners.clone();
            ui.people_offer = Some(offer);
        }
        let text = if self.stopping.is_some() {
            "Mesh · Stopping…"
        } else if self.error.is_some() {
            "Mesh · Needs attention — Settings for details"
        } else if !self.snapshot.running {
            "Mesh · Getting ready…"
        } else if self.snapshot.models_available {
            "Mesh · Models available"
        } else if self.snapshot.local_model_pending {
            "Mesh · Preparing local model (first download may take a while)…"
        } else {
            "Mesh · Finding models…"
        };
        ui.status.set_text(text);
        ui.public.set_enabled(self.stopping.is_none());
        ui.private.set_enabled(self.stopping.is_none());
        let retry = self.error.is_some() && self.child.is_none();
        if retry != ui.retry_visible {
            if retry {
                let _ = ui.menu.insert(&ui.retry, 4);
            } else {
                let _ = ui.menu.remove(&ui.retry);
            }
            ui.retry_visible = retry;
        }
        ui.quit.set_enabled(self.stopping.is_none());
    }

    fn open(&mut self, path: &'static str) {
        if self.child.is_none() || self.error.is_some() {
            let log = self.root.join("mesh.log");
            let message = self.error.as_deref().unwrap_or("Mesh is not running");
            eprintln!("{message}");
            // Native text viewer, not a second settings application. Works even
            // when startup failed before the management server could exist.
            let details = self.root.join("STARTUP_ERROR.txt");
            let body = format!(
                "Mesh needs attention\n\n{message}\n\nUse Retry startup after fixing the installation.\nRuntime log: {}\n",
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
        self.pending_settings = None;
        self.offer_reply = false;
        self.offer_membership_card = false;
        let Some(child) = &mut self.child else {
            self.exit = true;
            return;
        };
        match lifecycle::request_stop(child, self.settings.console_port) {
            Ok(()) => self.stopping = Some(Instant::now()),
            Err(e) => self.error = Some(e),
        }
    }

    fn tick(&mut self) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            match event.id.as_ref() {
                "chat" => self.open("/chat"),
                "settings" => self.open("/configuration/mesh"),
                "public" => self.change_mode(settings::Connection::Automatic),
                "private" => self.change_mode(settings::Connection::Private { invite: None }),
                "invite-member" => self.invite_member(),
                "share-membership" => self.share_membership(),
                "paste-card" => self.paste_card(),
                "share-request" => self.share_request(),
                "cancel-requests" => self.cancel_requests(),
                "share-reply" => self.share_reply(),

                "retry" => self.start(),
                "reset" => self.reset(),
                "quit" => self.quit(),
                id if id.starts_with("remove:") => self.remove_person(&id[7..]),
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
                if self.offer_reply && self.snapshot.private_owner.is_some() {
                    self.offer_reply = false;
                    self.share_reply();
                }
                if self.offer_membership_card && self.snapshot.private_owner.is_some() {
                    self.offer_membership_card = false;
                    self.share_membership();
                }
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
                            "Mesh exited ({code}). Open Settings for the startup log, then Retry startup."
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
                "Mesh startup timed out. Open Settings for the log; Quit stops only this instance."
                    .into(),
            );
        }
        if !self.polling && Instant::now() >= self.next_poll {
            self.polling = self.tx.send(()).is_ok();
            self.next_poll = Instant::now() + POLL;
        }
        self.render();
    }
    fn change_mode(&mut self, connection: settings::Connection) {
        // muda toggles the clicked item before dispatch. Keep the committed mode
        // visible through confirmation, cancellation and busy/same-mode returns.
        self.restore_mode_checks();
        if self.stopping.is_some()
            || self.pending_settings.is_some()
            || std::mem::discriminant(&self.settings.connection)
                == std::mem::discriminant(&connection)
        {
            return;
        }
        if !native::confirm(
            "Change Mesh connection?",
            "Restarts this app’s Mesh and cancels anything outstanding.",
            "Change connection",
        ) {
            return;
        }
        if matches!(connection, settings::Connection::Private { .. }) {
            if let Err(e) = identity::establish(&self.root) {
                native::notice("Could not set up Private", &e);
                return;
            }
        }
        match consent::cancel(&self.settings) {
            Ok(mut next) => {
                next.connection = connection;
                self.queue_settings(next);
            }
            Err(e) => native::notice("Could not change connection", &e),
        }
    }

    fn queue_settings(&mut self, next: settings::Settings) {
        if self.stopping.is_some() || self.pending_settings.is_some() {
            return;
        }
        self.pending_settings = Some(next);
        self.open_when_ready = None;
        if let Some(child) = &mut self.child {
            match lifecycle::request_stop(child, self.settings.console_port) {
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

    /// Forget this tray's identity and pairings, keeping downloaded models.
    /// The runtime is stopped first so it cannot rewrite state we just cleared.
    fn reset(&mut self) {
        if self.stopping.is_some() || self.pending_settings.is_some() || self.pending_reset {
            return;
        }
        if !native::confirm(
            "Start over on this Mesh?",
            "This tray forgets its own Mesh identity and every person it has paired with, so you will need to pair again. Downloaded models, your other Mesh nodes and your Buzz data are not touched.",
            "Start Over",
        ) {
            return;
        }
        self.pending_reset = true;
        self.open_when_ready = None;
        if let Some(child) = &mut self.child {
            match lifecycle::request_stop(child, self.settings.console_port) {
                Ok(()) => self.stopping = Some(Instant::now()),
                Err(e) => {
                    self.error = Some(e);
                    self.pending_reset = false;
                }
            }
        } else {
            self.apply_pending();
        }
    }

    fn apply_pending(&mut self) {
        if std::mem::take(&mut self.pending_reset) {
            match mesh_tray::reset::perform(&self.root) {
                Ok(()) => match settings::Settings::load(&self.root) {
                    Ok(settings) => {
                        self.settings = settings;
                        self.snapshot = status::Snapshot::default();
                        self.error = None;
                        self.start();
                    }
                    Err(e) => self.error = Some(format!("Could not reload settings: {e}")),
                },
                Err(e) => self.error = Some(format!("Could not start over: {e}")),
            }
            return;
        }
        if let Some(next) = self.pending_settings.take() {
            match next.save(&self.root) {
                Ok(()) => {
                    self.offer_reply = !next.replies.is_empty();
                    self.settings = next;
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
            for (label, route) in [
                ("Open Chat", Some("/chat")),
                ("Settings", Some("/configuration/mesh")),
                ("Quit Mesh", None),
            ] {
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
                ("Request to join", "request"),
                ("Accept an invitation or RSVP", "paste"),
                ("Share approved reply", "reply"),
                ("Cancel pending", "cancel"),
                ("Retry startup", "retry"),
                ("Start over", "reset"),
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
                        "request" => app.share_request(),
                        "paste" => app.paste_card(),
                        "reply" => app.share_reply(),
                        "cancel" => app.cancel_requests(),
                        "retry" => app.start(),
                        "reset" => app.reset(),
                        _ => {}
                    }
                });
                content.add(&button);
            }
            let people = gtk::MenuButton::new();
            people.set_label("People allowed");
            let popover = gtk::Popover::new(Some(&people));
            let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
            popover.add(&list);
            people.set_popover(Some(&popover));
            let state = app.clone();
            people.connect_toggled(move |button| {
                if !button.is_active() {
                    return;
                }
                for child in list.children() {
                    list.remove(&child);
                }
                let Ok(state_ref) = state.try_borrow() else {
                    return;
                };
                for owner in &state_ref.settings.admitted_owners {
                    let name = state_ref
                        .settings
                        .owner_names
                        .get(owner)
                        .map(String::as_str)
                        .unwrap_or("Mesh person");
                    let remove = gtk::Button::with_label(&format!("{name} · {}…", &owner[..12]));
                    let owner = owner.clone();
                    let state = state.clone();
                    remove.connect_clicked(move |_| {
                        if let Ok(mut app) = state.try_borrow_mut() {
                            app.remove_person(&owner);
                        }
                    });
                    list.add(&remove);
                }
                if state_ref.settings.admitted_owners.is_empty() {
                    list.add(&gtk::Label::new(Some("No other members yet")));
                }
                list.show_all();
            });
            content.add(&people);
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

fn main() {
    let result = (|| {
        let root = settings::data_root()?;
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let _profile_lock = identity::lock_profile(&root)?;
        let settings = settings::Settings::load(&root)?;
        if std::env::args().any(|arg| arg == "--print-launch") {
            // Omit private invitation material from diagnostic output.
            println!(
                "mode={} console={} api={}",
                if matches!(settings.connection, settings::Connection::Automatic) {
                    "automatic"
                } else {
                    "private"
                },
                settings.console_port,
                settings.api_port
            );
            return Ok(());
        }
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
        let mut adapter = portable::Native::default();
        let _ = &mut adapter;
        let _ = portable::Native::share;
        // Clipboard access is the one API that cannot be checked here: the
        // portable adapter's implementation is GTK, gated to Linux.
        let _ = portable::notice;
        let _ = portable::confirm;
        let _ = portable::decision;
    }
    use super::*;
    fn app(root: &std::path::Path) -> App {
        App::new(root.into(), settings::Settings::default())
    }
    #[test]
    fn failed_save_leaves_current_memory_and_disk_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        std::fs::create_dir(root.path().join("launcher.json")).unwrap();
        let mut candidate = app.settings.clone();
        candidate.admitted_owners.push("ab".repeat(32));
        app.pending_settings = Some(candidate);
        app.apply_pending();
        assert!(app.settings.admitted_owners.is_empty());
        assert!(app.child.is_none());
        assert!(app.error.as_ref().unwrap().contains("save"));
        assert!(app.pending_settings.is_none());
    }
    #[test]
    fn startup_failure_keeps_committed_removal_never_restores_old_grants() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        let owner = "ab".repeat(32);
        app.settings.admitted_owners.push(owner.clone());
        app.settings.save(root.path()).unwrap();
        // Deterministic occupied-port failure, never start a Mesh process.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        app.settings.console_port = listener.local_addr().unwrap().port();
        app.settings.api_port = if app.settings.console_port == 9447 {
            9448
        } else {
            9447
        };
        let next = consent::remove(&app.settings, &owner, 0).unwrap();
        app.pending_settings = Some(next);
        app.apply_pending();
        assert!(app.child.is_none());
        assert!(app.error.is_some());
        assert!(app.settings.admitted_owners.is_empty());
        assert!(settings::Settings::load(root.path())
            .unwrap()
            .admitted_owners
            .is_empty());
        assert_eq!(app.settings.exchange.generation(), 1);
        app.start();
        assert!(app.settings.admitted_owners.is_empty());
    }
    #[cfg(unix)]
    fn exited_child() -> Child {
        let mut child = Command::new("sh").args(["-c", "exit 7"]).spawn().unwrap();
        // try_wait returns the cached status too; no scheduling race in tick.
        child.wait().unwrap();
        child
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
        // Block on our open pipe, not a sleep or a real Mesh/status service.
        let child = Command::new("sh")
            .args(["-c", "read -r line"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        app.child = Some(child);
        app.started = Some(Instant::now());

        app.tick();
        let fresh_start_pending = app.started.is_some() && app.error.is_none();
        app.started = Some(Instant::now() - Duration::from_secs(181));
        app.tick();
        let retained_pid = app.child.as_ref().map(Child::id);
        // Reap the fixture before assertions; wait closes stdin so read sees EOF.
        let mut child = app.child.take().unwrap();
        let still_alive = child.try_wait().unwrap().is_none();
        child.wait().unwrap();

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
    fn consent_is_not_saved_until_owned_child_is_reaped_and_busy_actions_do_not_replace_it() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        app.settings.save(root.path()).unwrap();
        let mut other = Command::new("sleep").arg("30").spawn().unwrap();
        app.child = Some(Command::new("sleep").arg("30").spawn().unwrap());
        let mut next = app.settings.clone();
        next.admitted_owners.push("ab".repeat(32));
        app.queue_settings(next);
        assert!(settings::Settings::load(root.path())
            .unwrap()
            .admitted_owners
            .is_empty());
        app.queue_settings(settings::Settings::default());
        assert_eq!(
            app.pending_settings.as_ref().unwrap().admitted_owners.len(),
            1
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.child.as_mut().unwrap().try_wait().unwrap().is_none() && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut child = app.child.take().unwrap();
        let _ = child.kill();
        child.wait().unwrap();
        app.stopping = None;
        // Force save failure so this test cannot spawn a runtime after reaping.
        std::fs::remove_file(root.path().join("launcher.json")).unwrap();
        std::fs::create_dir(root.path().join("launcher.json")).unwrap();
        app.apply_pending();
        assert!(app.settings.admitted_owners.is_empty());
        assert!(app.child.is_none());
        let alive = other.try_wait().unwrap().is_none();
        other.kill().unwrap();
        other.wait().unwrap();
        assert!(alive);
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "native_portable.rs"]
mod portable_test;

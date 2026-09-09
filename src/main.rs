#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod lifecycle;
mod settings;
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
    requests: MenuItem,
    requests_visible: bool,
    retry: MenuItem,
    retry_visible: bool,
    quit: MenuItem,
    _tray: TrayIcon,
}

struct App {
    settings: settings::Settings,
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
    error: Option<String>,
    open_when_ready: Option<&'static str>,
    exit: bool,
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
            open_when_ready: Some("/chat"),
            exit: false,
        }
    }

    fn build(&mut self) -> Result<(), String> {
        let menu = Menu::new();
        let status = MenuItem::new("Mesh · Getting ready…", false, None);
        let chat = MenuItem::with_id("chat", "Open Chat…", true, None);
        let settings = MenuItem::with_id("settings", "Settings…", true, None);
        let quit = MenuItem::with_id("quit", "Quit Mesh", true, None);
        let requests = MenuItem::with_id("requests", "Review connection request…", true, None);
        let retry = MenuItem::with_id("retry", "Retry startup…", true, None);
        menu.append_items(&[
            &status,
            &PredefinedMenuItem::separator(),
            &chat,
            &settings,
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
            requests,
            requests_visible: false,
            retry,
            retry_visible: false,
            quit,
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
        // A separate home isolates identity, configuration, models and runtime GC
        // from CLI/lab instances. It persists across launches of this app.
        let home = self.root.join("home");
        std::fs::create_dir_all(home.join(".mesh-llm")).map_err(|e| e.to_string())?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("mesh.log"))
            .map_err(|e| e.to_string())?;
        let mut command = Command::new(binary);
        command
            .args(self.settings.args())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log))
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("MESH_LLM_CONFIG", home.join(".mesh-llm/config.toml"))
            .env("MESH_LLM_RUNTIME_ROOT", self.root.join("runtime"))
            .env_remove("MESH_LLM_EPHEMERAL_KEY");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        command
            .spawn()
            .map_err(|e| format!("Could not start Mesh: {e}"))
    }

    fn render(&mut self) {
        let Some(ui) = &mut self.ui else { return };
        let text = if self.stopping.is_some() {
            "Mesh · Stopping…"
        } else if self.error.is_some() {
            "Mesh · Needs attention — Settings for details"
        } else if !self.snapshot.running {
            "Mesh · Getting ready…"
        } else if self.snapshot.models_available {
            "Mesh · Models available"
        } else {
            "Mesh · Finding models…"
        };
        ui.status.set_text(text);
        let pending = self.snapshot.pending > 0 && self.child.is_some();
        ui.requests.set_text(format!(
            "Review connection requests ({})…",
            self.snapshot.pending
        ));
        if pending != ui.requests_visible {
            if pending {
                let _ = ui.menu.insert(&ui.requests, 4);
            } else {
                let _ = ui.menu.remove(&ui.requests);
            }
            ui.requests_visible = pending;
        }
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
            let body = format!("Mesh needs attention\n\n{message}\n\nUse Retry startup in the tray after fixing the installation.\nRuntime log: {}\n", log.display());
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
                "requests" => self.open("/#pairing"),
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
                    self.snapshot = status::Snapshot::default();
                    if self.stopping.take().is_some() {
                        self.exit = true;
                    } else {
                        self.error = Some(format!("Mesh exited ({code}). Open Settings for the startup log, then Retry startup."));
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
        builder
            .build()
            .map_err(|e| e.to_string())?
            .run_app(&mut app)
            .map_err(|e| e.to_string())
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
        // GNOME without an extension must never leave an invisible daemon.
        let window = if fallback {
            let window = gtk::Window::new(gtk::WindowType::Toplevel);
            window.set_title("Mesh");
            window.set_default_size(320, 180);
            let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
            content.set_border_width(24);
            content.add(&gtk::Label::new(Some("Mesh runs until you quit.")));
            for (label, route) in [
                ("Open Chat", Some("/chat")),
                ("Settings", Some("/configuration/mesh")),
                ("Quit Mesh", None),
            ] {
                let button = gtk::Button::with_label(label);
                let app = app.clone();
                button.connect_clicked(move |_| {
                    let mut app = app.borrow_mut();
                    if let Some(path) = route {
                        app.open(path);
                    } else {
                        app.quit();
                    }
                });
                content.add(&button);
            }
            window.add(&content);
            let closing = app.clone();
            window.connect_delete_event(move |_, _| {
                closing.borrow_mut().quit();
                gtk::glib::Propagation::Stop
            });
            window.show_all();
            Some(window)
        } else {
            None
        };
        gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
            let _ = &window;
            let mut app = app.borrow_mut();
            app.tick();
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

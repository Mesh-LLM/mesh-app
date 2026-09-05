mod api;

use muda::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};

const POLL: Duration = Duration::from_secs(2);

fn console_port() -> u16 {
    std::env::var("MESH_LLM_CONSOLE_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3131)
}

fn mesh_bin() -> String {
    std::env::var("MESH_LLM_BIN").unwrap_or_else(|_| "mesh-llm".to_string())
}

fn console_url(port: u16) -> String {
    format!("http://localhost:{port}")
}

struct App {
    port: u16,
    tray: Option<TrayIcon>,
    /// menu id -> action
    actions: HashMap<MenuId, Action>,
    next_poll: Instant,
}

#[derive(Clone)]
enum Action {
    OpenConsole,
    Pair,
    Start,
    Stop,
    Quit,
    Approve(String),
    Reject(String),
}

fn icon() -> Icon {
    // 16x16 filled rounded square, template-ish monochrome.
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u32 {
        for x in 0..16u32 {
            let edge = x == 0 || y == 0 || x == 15 || y == 15;
            let corner = !(2..=13).contains(&x) && !(2..=13).contains(&y);
            let a = if corner {
                0
            } else if edge {
                160
            } else {
                255
            };
            rgba.extend_from_slice(&[30, 30, 30, a]);
        }
    }
    Icon::from_rgba(rgba, 16, 16).expect("icon")
}

impl App {
    fn new(port: u16) -> Self {
        Self {
            port,
            tray: None,
            actions: HashMap::new(),
            next_poll: Instant::now(),
        }
    }

    fn build_menu(&mut self, snap: &api::Snapshot) -> Menu {
        self.actions.clear();
        let menu = Menu::new();
        let header = if snap.running {
            format!("Mesh — running · {} peers", snap.peers.len())
        } else {
            "Mesh — not running".to_string()
        };
        let status = MenuItem::new(header, false, None);
        menu.append(&status).ok();
        menu.append(&PredefinedMenuItem::separator()).ok();

        if snap.running && snap.pairing_supported {
            if snap.pending.is_empty() {
                menu.append(&MenuItem::new("No connection requests", false, None))
                    .ok();
            } else {
                for session in &snap.pending {
                    let code = session
                        .comparison_code
                        .clone()
                        .unwrap_or_else(|| "no code".into());
                    let approve = MenuItem::new(
                        format!("Approve \"{}\" · {code}", session.peer_name),
                        true,
                        None,
                    );
                    let reject =
                        MenuItem::new(format!("Reject \"{}\"", session.peer_name), true, None);
                    self.actions
                        .insert(approve.id().clone(), Action::Approve(session.id.clone()));
                    self.actions
                        .insert(reject.id().clone(), Action::Reject(session.id.clone()));
                    menu.append(&approve).ok();
                    menu.append(&reject).ok();
                }
            }
            menu.append(&PredefinedMenuItem::separator()).ok();
        }

        if snap.running {
            let members = Submenu::new("Members", true);
            if snap.peers.is_empty() {
                members
                    .append(&MenuItem::new("this device only", false, None))
                    .ok();
            } else {
                for peer in &snap.peers {
                    members.append(&MenuItem::new(peer, false, None)).ok();
                }
            }
            menu.append(&members).ok();
            menu.append(&PredefinedMenuItem::separator()).ok();
        }

        let open = MenuItem::new("Open Console", snap.running, None);
        self.actions.insert(open.id().clone(), Action::OpenConsole);
        menu.append(&open).ok();
        if snap.pairing_supported {
            let pair = MenuItem::new("Pair a device…", true, None);
            self.actions.insert(pair.id().clone(), Action::Pair);
            menu.append(&pair).ok();
        }
        menu.append(&PredefinedMenuItem::separator()).ok();

        let start = MenuItem::new("Start Mesh", !snap.running, None);
        let stop = MenuItem::new("Stop Mesh", snap.running, None);
        self.actions.insert(start.id().clone(), Action::Start);
        self.actions.insert(stop.id().clone(), Action::Stop);
        menu.append(&start).ok();
        menu.append(&stop).ok();
        menu.append(&PredefinedMenuItem::separator()).ok();
        let quit = MenuItem::new("Quit", true, None);
        self.actions.insert(quit.id().clone(), Action::Quit);
        menu.append(&quit).ok();
        menu
    }

    fn refresh(&mut self) {
        let snap = api::snapshot(self.port);
        let menu = self.build_menu(&snap);
        match self.tray.as_mut() {
            Some(tray) => {
                tray.set_menu(Some(Box::new(menu)));
                tray.set_tooltip(Some(if snap.running {
                    format!("Mesh · {} peers", snap.peers.len())
                } else {
                    "Mesh · stopped".to_string()
                }))
                .ok();
            }
            None => {
                let tray = TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_icon(icon())
                    .with_tooltip("Mesh")
                    .build()
                    .expect("tray icon");
                self.tray = Some(tray);
            }
        }
    }

    fn start_mesh(&self) {
        let result = Command::new(mesh_bin())
            .arg("serve")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Err(error) = result {
            eprintln!("could not start mesh: {error}");
        }
    }

    fn handle(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::OpenConsole => {
                let _ = open::that(console_url(self.port));
            }
            Action::Pair => {
                let _ = open::that(format!("{}/#pairing", console_url(self.port)));
            }
            Action::Start => self.start_mesh(),
            Action::Stop => {
                if let Err(error) = api::shutdown(self.port) {
                    eprintln!("stop failed: {error}");
                }
            }
            Action::Approve(id) => {
                if let Err(error) = api::decide(self.port, &id, "approve") {
                    eprintln!("approve failed: {error}");
                }
            }
            Action::Reject(id) => {
                if let Err(error) = api::decide(self.port, &id, "reject") {
                    eprintln!("reject failed: {error}");
                }
            }
            Action::Quit => event_loop.exit(),
        }
        self.next_poll = Instant::now();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        _event: winit::event::WindowEvent,
    ) {
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = self.actions.get(&event.id).cloned() {
                self.handle(action, event_loop);
            }
        }
        if Instant::now() >= self.next_poll {
            self.refresh();
            self.next_poll = Instant::now() + POLL;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_poll));
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    let event_loop = {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        EventLoop::builder()
            .with_activation_policy(ActivationPolicy::Accessory)
            .build()
            .expect("event loop")
    };
    #[cfg(not(target_os = "macos"))]
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(console_port());
    event_loop.run_app(&mut app).expect("run");
}

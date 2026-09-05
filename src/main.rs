mod api;

use muda::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};

/// Poll interval. Deliberately slow: the only thing that needs to be fresh is
/// an incoming pairing request.
const POLL: Duration = Duration::from_secs(3);
/// Fixed number of pending-request rows. The top-level menu structure is built
/// ONCE and never rebuilt: replacing a live menu closes it mid-click, and
/// dropping the old items lets muda recycle their ids, so a stale id -> Action
/// mapping can fire the WRONG action (this is why "Open Console" quit the app).
const PENDING_SLOTS: usize = 4;

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

#[derive(Clone)]
enum Action {
    OpenConsole,
    Pair,
    Start,
    Stop,
    Quit,
    Approve(usize),
    Reject(usize),
}

/// Every menu item the tray owns, created once at startup.
struct Ui {
    status: MenuItem,
    no_requests: MenuItem,
    approve: Vec<MenuItem>,
    reject: Vec<MenuItem>,
    members: Submenu,
    open: MenuItem,
    pair: MenuItem,
    start: MenuItem,
    stop: MenuItem,
}

struct App {
    port: u16,
    tray: Option<TrayIcon>,
    ui: Option<Ui>,
    actions: HashMap<MenuId, Action>,
    /// session id currently shown in each pending slot
    slots: Vec<Option<String>>,
    /// last rendered members list, to avoid touching the submenu unnecessarily
    rendered_members: Vec<String>,
    next_poll: Instant,
}

/// 16x16 solid glyph. Rendered as a macOS template image so the system
/// recolours it for light/dark menu bars — a fixed dark grey is invisible on a
/// dark menu bar, which is why the first version showed nothing.
fn icon() -> Icon {
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16i32 {
        for x in 0..16i32 {
            let (dx, dy) = (x - 8, y - 8);
            let d2 = dx * dx + dy * dy;
            // filled centre dot plus a ring: a small "mesh node" mark
            // filled centre dot OR the surrounding ring
            let alpha = if d2 <= 6 || (30..=56).contains(&d2) {
                255
            } else {
                0
            };
            rgba.extend_from_slice(&[0, 0, 0, alpha]);
        }
    }
    Icon::from_rgba(rgba, 16, 16).expect("icon")
}

/// The complete id -> action table, built without touching any UI object so it
/// is testable off the main thread. Ids are explicit and stable.
fn action_table() -> Vec<(String, Action)> {
    let mut table = vec![
        ("open".to_string(), Action::OpenConsole),
        ("pair".to_string(), Action::Pair),
        ("start".to_string(), Action::Start),
        ("stop".to_string(), Action::Stop),
        ("quit".to_string(), Action::Quit),
    ];
    for slot in 0..PENDING_SLOTS {
        table.push((format!("approve-{slot}"), Action::Approve(slot)));
        table.push((format!("reject-{slot}"), Action::Reject(slot)));
    }
    table
}

impl App {
    fn new(port: u16) -> Self {
        Self {
            port,
            tray: None,
            ui: None,
            actions: HashMap::new(),
            slots: vec![None; PENDING_SLOTS],
            rendered_members: Vec::new(),
            next_poll: Instant::now(),
        }
    }

    fn build(&mut self) -> Menu {
        let menu = Menu::new();
        // Explicit, stable ids. Never rely on muda's auto-assigned ids for an
        // action: they are recycled when an item is dropped.
        let status = MenuItem::with_id("status", "Mesh — checking…", false, None);
        let no_requests = MenuItem::with_id("no-requests", "No connection requests", false, None);
        let members = Submenu::with_id("members", "Members", true);
        let open = MenuItem::with_id("open", "Open Console", false, None);
        let pair = MenuItem::with_id("pair", "Pair a device…", false, None);
        let start = MenuItem::with_id("start", "Start Mesh", true, None);
        let stop = MenuItem::with_id("stop", "Stop Mesh", false, None);
        let quit = MenuItem::with_id("quit", "Quit", true, None);

        let mut approve = Vec::new();
        let mut reject = Vec::new();
        for slot in 0..PENDING_SLOTS {
            approve.push(MenuItem::with_id(
                format!("approve-{slot}"),
                "",
                false,
                None,
            ));
            reject.push(MenuItem::with_id(format!("reject-{slot}"), "", false, None));
        }
        for (id, action) in action_table() {
            self.actions.insert(MenuId::new(id), action);
        }

        menu.append(&status).ok();
        menu.append(&PredefinedMenuItem::separator()).ok();
        menu.append(&no_requests).ok();
        for slot in 0..PENDING_SLOTS {
            menu.append(&approve[slot]).ok();
            menu.append(&reject[slot]).ok();
        }
        menu.append(&PredefinedMenuItem::separator()).ok();
        menu.append(&members).ok();
        menu.append(&PredefinedMenuItem::separator()).ok();
        menu.append(&open).ok();
        menu.append(&pair).ok();
        menu.append(&PredefinedMenuItem::separator()).ok();
        menu.append(&start).ok();
        menu.append(&stop).ok();
        menu.append(&PredefinedMenuItem::separator()).ok();
        menu.append(&quit).ok();

        self.ui = Some(Ui {
            status,
            no_requests,
            approve,
            reject,
            members,
            open,
            pair,
            start,
            stop,
        });
        menu
    }

    /// Update labels and enabled state IN PLACE. Never replaces the menu.
    fn apply(&mut self, snap: &api::Snapshot) {
        let Some(ui) = self.ui.as_ref() else { return };

        ui.status.set_text(if snap.running {
            format!("Mesh — running · {} peers", snap.peers.len())
        } else {
            "Mesh — not running".to_string()
        });

        let show_pending = snap.running && snap.pairing_supported;
        ui.no_requests
            .set_text(if !snap.pairing_supported && snap.running {
                "Pairing needs a newer mesh-llm".to_string()
            } else if snap.pending.is_empty() {
                "No connection requests".to_string()
            } else {
                format!("{} connection request(s)", snap.pending.len())
            });

        for slot in 0..PENDING_SLOTS {
            match snap.pending.get(slot).filter(|_| show_pending) {
                Some(session) => {
                    let code = session.comparison_code.as_deref().unwrap_or("no code");
                    ui.approve[slot]
                        .set_text(format!("Approve \"{}\" · {code}", session.peer_name));
                    ui.reject[slot].set_text(format!("Reject \"{}\"", session.peer_name));
                    ui.approve[slot].set_enabled(true);
                    ui.reject[slot].set_enabled(true);
                    self.slots[slot] = Some(session.id.clone());
                }
                None => {
                    // muda cannot hide an item, so unused slots collapse to a
                    // zero-width disabled separator-ish row.
                    ui.approve[slot].set_text("");
                    ui.reject[slot].set_text("");
                    ui.approve[slot].set_enabled(false);
                    ui.reject[slot].set_enabled(false);
                    self.slots[slot] = None;
                }
            }
        }

        ui.open.set_enabled(snap.running);
        ui.pair.set_enabled(snap.running && snap.pairing_supported);
        ui.start.set_enabled(!snap.running);
        ui.stop.set_enabled(snap.running);

        // Members submenu children are the only thing that must be recreated.
        // Gate on change so an open menu is not disturbed on every poll.
        if snap.peers != self.rendered_members {
            while ui.members.remove_at(0).is_some() {}
            if snap.peers.is_empty() {
                ui.members
                    .append(&MenuItem::with_id(
                        "member-none",
                        if snap.running {
                            "this device only"
                        } else {
                            "mesh not running"
                        },
                        false,
                        None,
                    ))
                    .ok();
            } else {
                for (index, peer) in snap.peers.iter().enumerate() {
                    ui.members
                        .append(&MenuItem::with_id(
                            format!("member-{index}"),
                            peer,
                            false,
                            None,
                        ))
                        .ok();
                }
            }
            self.rendered_members = snap.peers.clone();
        }
    }

    fn refresh(&mut self) {
        let snap = api::snapshot(self.port);
        if self.tray.is_none() {
            let menu = self.build();
            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_icon(icon())
                .with_icon_as_template(true)
                .with_tooltip("Mesh")
                .build()
                .expect("tray icon");
            self.tray = Some(tray);
        }
        self.apply(&snap);
        if let Some(tray) = self.tray.as_ref() {
            // A short title guarantees the item is visible even if the icon
            // fails to render on some desktop.
            tray.set_title(Some(if snap.running {
                format!("Mesh {}", snap.peers.len())
            } else {
                "Mesh –".to_string()
            }));
        }
    }

    fn start_mesh(&self) {
        if let Err(error) = Command::new(mesh_bin())
            .arg("serve")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
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
            Action::Approve(slot) => self.decide(slot, "approve"),
            Action::Reject(slot) => self.decide(slot, "reject"),
            Action::Quit => event_loop.exit(),
        }
        self.next_poll = Instant::now();
    }

    fn decide(&self, slot: usize, decision: &str) {
        let Some(Some(id)) = self.slots.get(slot) else {
            return;
        };
        if let Err(error) = api::decide(self.port, id, decision) {
            eprintln!("{decision} failed: {error}");
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard for the bug where "Open Console" quit the app: every
    /// action must be reachable under its own distinct, explicit id.
    #[test]
    fn every_action_has_a_distinct_explicit_id() {
        let ids: Vec<String> = action_table().into_iter().map(|(id, _)| id).collect();
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "duplicate menu action id: {ids:?}");
        for expected in ["open", "pair", "start", "stop", "quit"] {
            assert!(
                ids.iter().any(|id| id == expected),
                "missing action id {expected} in {ids:?}"
            );
        }
        assert_eq!(ids.len(), 5 + PENDING_SLOTS * 2);
        let actions: HashMap<MenuId, Action> = action_table()
            .into_iter()
            .map(|(id, action)| (MenuId::new(id), action))
            .collect();
        assert!(matches!(
            actions.get(&MenuId::new("open")),
            Some(Action::OpenConsole)
        ));
        assert!(matches!(
            actions.get(&MenuId::new("quit")),
            Some(Action::Quit)
        ));
        assert!(matches!(
            actions.get(&MenuId::new("approve-2")),
            Some(Action::Approve(2))
        ));
    }

    #[test]
    fn empty_pending_slots_hold_no_session_id() {
        let app = App::new(3131);
        assert_eq!(app.slots.len(), PENDING_SLOTS);
        assert!(app.slots.iter().all(Option::is_none));
    }
}

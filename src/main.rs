mod api;

use muda::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};

/// Poll interval. Deliberately slow: the only thing that needs to be fresh is
/// an incoming pairing request.
const POLL: Duration = Duration::from_secs(3);
/// Fixed number of rows for each dynamic list. The menu structure is built
/// ONCE and never rebuilt: replacing a live menu closes it mid-click, and
/// dropping the old items lets muda recycle their ids, so a stale id -> Action
/// mapping can fire the WRONG action (this is why "Open Console" quit the app).
const PENDING_SLOTS: usize = 4;
const MODEL_SLOTS: usize = 12;
const NEARBY_SLOTS: usize = 6;

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

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    OpenConsole,
    Pair,
    Start,
    Stop,
    Quit,
    Approve(usize),
    Reject(usize),
    /// Toggle: load if not serving, unload if serving.
    Model(usize),
    Discover,
    /// Join a discovered mesh by restarting the daemon with its invite token.
    Join(usize),
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
        ("discover".to_string(), Action::Discover),
    ];
    for slot in 0..PENDING_SLOTS {
        table.push((format!("approve-{slot}"), Action::Approve(slot)));
        table.push((format!("reject-{slot}"), Action::Reject(slot)));
    }
    for slot in 0..MODEL_SLOTS {
        table.push((format!("model-{slot}"), Action::Model(slot)));
    }
    for slot in 0..NEARBY_SLOTS {
        table.push((format!("join-{slot}"), Action::Join(slot)));
    }
    table
}

/// Every menu item the tray owns, created once at startup.
struct Ui {
    status: MenuItem,
    no_requests: MenuItem,
    approve: Vec<MenuItem>,
    reject: Vec<MenuItem>,
    members: Submenu,
    models: Submenu,
    model_rows: Vec<MenuItem>,
    nearby: Submenu,
    discover: MenuItem,
    nearby_rows: Vec<MenuItem>,
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
    /// (model ref, currently serving) per model slot
    model_slots: Vec<Option<(String, bool)>>,
    nearby_slots: Vec<Option<api::DiscoveredMesh>>,
    control_endpoint: Option<String>,
    /// The `mesh-llm serve` child we spawned, so Stop works even on daemons
    /// without `POST /api/runtime/shutdown` (released 0.76.0-rc9 404s it).
    child: Option<Child>,
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

/// Trim a model ref to something that fits a menu row.
fn short_model(model: &str) -> String {
    let tail = model.rsplit('/').next().unwrap_or(model);
    if tail.len() > 42 {
        format!("{}…", &tail[..41])
    } else {
        tail.to_string()
    }
}

impl App {
    fn new(port: u16) -> Self {
        Self {
            port,
            tray: None,
            ui: None,
            actions: HashMap::new(),
            slots: vec![None; PENDING_SLOTS],
            model_slots: vec![None; MODEL_SLOTS],
            nearby_slots: vec![None; NEARBY_SLOTS],
            control_endpoint: None,
            child: None,
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
        let models = Submenu::with_id("models", "Models", true);
        let nearby = Submenu::with_id("nearby", "Nearby meshes", true);
        let discover = MenuItem::with_id("discover", "Look for meshes…", true, None);
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
        let mut model_rows = Vec::new();
        for slot in 0..MODEL_SLOTS {
            let row = MenuItem::with_id(format!("model-{slot}"), "", false, None);
            models.append(&row).ok();
            model_rows.push(row);
        }
        let mut nearby_rows = Vec::new();
        nearby.append(&discover).ok();
        nearby.append(&PredefinedMenuItem::separator()).ok();
        for slot in 0..NEARBY_SLOTS {
            let row = MenuItem::with_id(format!("join-{slot}"), "", false, None);
            nearby.append(&row).ok();
            nearby_rows.push(row);
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
        menu.append(&models).ok();
        menu.append(&nearby).ok();
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
            models,
            model_rows,
            nearby,
            discover,
            nearby_rows,
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
            .set_text(if snap.running && !snap.pairing_supported {
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
                    // zero-width disabled row.
                    ui.approve[slot].set_text("");
                    ui.reject[slot].set_text("");
                    ui.approve[slot].set_enabled(false);
                    ui.reject[slot].set_enabled(false);
                    self.slots[slot] = None;
                }
            }
        }

        // Models: serving ones first, marked, and clicking toggles.
        self.control_endpoint = snap.control_endpoint.clone();
        let mut listed: Vec<(String, bool)> = snap
            .serving_models
            .iter()
            .map(|model| (model.clone(), true))
            .collect();
        for model in &snap.available_models {
            if !snap.serving_models.contains(model) {
                listed.push((model.clone(), false));
            }
        }
        let can_control = snap.running && snap.control_endpoint.is_some();
        ui.models.set_text(if snap.serving_models.is_empty() {
            "Models".to_string()
        } else {
            format!("Models · {} serving", snap.serving_models.len())
        });
        for slot in 0..MODEL_SLOTS {
            match listed.get(slot) {
                Some((model, serving)) => {
                    let mark = if *serving { "● " } else { "  " };
                    ui.model_rows[slot].set_text(format!("{mark}{}", short_model(model)));
                    ui.model_rows[slot].set_enabled(can_control);
                    self.model_slots[slot] = Some((model.clone(), *serving));
                }
                None => {
                    ui.model_rows[slot].set_text("");
                    ui.model_rows[slot].set_enabled(false);
                    self.model_slots[slot] = None;
                }
            }
        }
        ui.discover.set_enabled(snap.running);
        ui.nearby.set_text(if snap.discovery_mode.is_empty() {
            "Nearby meshes".to_string()
        } else {
            format!("Nearby meshes ({})", snap.discovery_mode)
        });

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

    fn start_mesh(&mut self, join_token: Option<&str>) {
        let mut command = Command::new(mesh_bin());
        command.arg("serve");
        if let Some(token) = join_token {
            command.arg("--join").arg(token);
        }
        match command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => self.child = Some(child),
            Err(error) => eprintln!("could not start mesh: {error}"),
        }
    }

    fn stop_mesh(&mut self) {
        // Graceful first; released daemons 404 the endpoint, so fall back to
        // terminating the child we spawned.
        if api::shutdown(self.port).is_ok() {
            self.child = None;
            return;
        }
        match self.child.as_mut() {
            Some(child) => {
                let _ = child.kill();
                let _ = child.wait();
                self.child = None;
            }
            None => eprintln!(
                "stop unavailable: this daemon has no /api/runtime/shutdown and was not started by the tray"
            ),
        }
    }

    fn set_nearby(&mut self, meshes: Vec<api::DiscoveredMesh>) {
        let Some(ui) = self.ui.as_ref() else { return };
        for slot in 0..NEARBY_SLOTS {
            match meshes.get(slot) {
                Some(mesh) => {
                    ui.nearby_rows[slot].set_text(format!("Join {}", mesh.label));
                    ui.nearby_rows[slot].set_enabled(true);
                    self.nearby_slots[slot] = Some(mesh.clone());
                }
                None => {
                    ui.nearby_rows[slot].set_text("");
                    ui.nearby_rows[slot].set_enabled(false);
                    self.nearby_slots[slot] = None;
                }
            }
        }
        ui.discover.set_text(if meshes.is_empty() {
            "No meshes found — look again".to_string()
        } else {
            format!("Found {} — look again", meshes.len())
        });
    }

    fn handle(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::OpenConsole => {
                let _ = open::that(console_url(self.port));
            }
            Action::Pair => {
                let _ = open::that(format!("{}/#pairing", console_url(self.port)));
            }
            Action::Start => self.start_mesh(None),
            Action::Stop => self.stop_mesh(),
            Action::Approve(slot) => self.decide(slot, "approve"),
            Action::Reject(slot) => self.decide(slot, "reject"),
            Action::Model(slot) => self.toggle_model(slot),
            Action::Discover => {
                let meshes = api::discover(self.port);
                self.set_nearby(meshes);
            }
            Action::Join(slot) => {
                if let Some(Some(mesh)) = self.nearby_slots.get(slot).cloned() {
                    // No management join endpoint exists; joining is a daemon
                    // start-up argument, so restart with the invite token.
                    self.stop_mesh();
                    self.start_mesh(Some(&mesh.invite_token));
                }
            }
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

    fn toggle_model(&self, slot: usize) {
        let Some(Some((model, serving))) = self.model_slots.get(slot) else {
            return;
        };
        let Some(endpoint) = self.control_endpoint.as_deref() else {
            eprintln!("no owner-control endpoint available");
            return;
        };
        let result = if *serving {
            api::unload_model(self.port, endpoint, model)
        } else {
            api::load_model(self.port, endpoint, model)
        };
        if let Err(error) = result {
            eprintln!("model {model} toggle failed: {error}");
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
        for expected in ["open", "pair", "start", "stop", "quit", "discover"] {
            assert!(
                ids.iter().any(|id| id == expected),
                "missing action id {expected} in {ids:?}"
            );
        }
        assert_eq!(
            ids.len(),
            6 + PENDING_SLOTS * 2 + MODEL_SLOTS + NEARBY_SLOTS
        );
        let actions: HashMap<MenuId, Action> = action_table()
            .into_iter()
            .map(|(id, action)| (MenuId::new(id), action))
            .collect();
        assert_eq!(
            actions.get(&MenuId::new("open")),
            Some(&Action::OpenConsole)
        );
        assert_eq!(actions.get(&MenuId::new("quit")), Some(&Action::Quit));
        assert_eq!(
            actions.get(&MenuId::new("approve-2")),
            Some(&Action::Approve(2))
        );
        assert_eq!(
            actions.get(&MenuId::new("model-11")),
            Some(&Action::Model(11))
        );
        assert_eq!(actions.get(&MenuId::new("join-5")), Some(&Action::Join(5)));
    }

    #[test]
    fn empty_slots_hold_nothing() {
        let app = App::new(3131);
        assert_eq!(app.slots.len(), PENDING_SLOTS);
        assert_eq!(app.model_slots.len(), MODEL_SLOTS);
        assert_eq!(app.nearby_slots.len(), NEARBY_SLOTS);
        assert!(app.slots.iter().all(Option::is_none));
        assert!(app.model_slots.iter().all(Option::is_none));
    }

    #[test]
    fn model_labels_are_trimmed_to_the_tail() {
        assert_eq!(
            short_model("unsloth/gemma-4-E4B-it-GGUF:Q4_K_M"),
            "gemma-4-E4B-it-GGUF:Q4_K_M"
        );
        let long = short_model("owner/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(long.chars().count(), 42);
        assert!(long.ends_with('…'));
    }
}

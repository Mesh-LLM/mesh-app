//! Bounded three-member admission probe against an official executable. No model
//! download: serve mode exercises full nodes, not client-only participants.
use mesh_tray::{
    admission, identity, invitation, runtime_home,
    settings::{Connection, Settings},
};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Node {
    child: Child,
    root: PathBuf,
    settings: Settings,
}
impl Drop for Node {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            #[cfg(unix)]
            unsafe {
                libc::kill(self.child.id() as i32, libc::SIGTERM);
            }
            let end = Instant::now() + Duration::from_secs(8);
            while Instant::now() < end {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
fn start(binary: &Path, root: &Path, settings: Settings) -> Node {
    let home = settings.runtime_home(root);
    runtime_home::prepare(&home).unwrap();
    admission::prepare_store(&home, &settings.admitted_owners).unwrap();
    settings.save(root).unwrap();
    let log = std::fs::File::create(root.join("probe-runtime.log")).unwrap();
    let mut command = Command::new(binary);
    command
        .args(settings.args())
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    runtime_home::configure(&mut command, &home);
    Node {
        child: command.spawn().unwrap(),
        root: root.into(),
        settings,
    }
}
fn status(node: &Node) -> Option<serde_json::Value> {
    let result = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build()
        .get(&format!(
            "http://127.0.0.1:{}/api/status",
            node.settings.console_port
        ))
        .call()
        .ok()?;
    let mut body = String::new();
    result.into_reader().read_to_string(&mut body).ok()?;
    serde_json::from_str(&body).ok()
}
fn wait(node: &mut Node, owner: &str, peers: usize) -> serde_json::Value {
    let end = Instant::now() + Duration::from_secs(75);
    loop {
        assert!(
            node.child.try_wait().unwrap().is_none(),
            "runtime exited; see {}",
            node.root.display()
        );
        if let Some(value) = status(node) {
            if value["owner"]["owner_id"] == owner
                && value["owner"]["verified"] == true
                && value["peers"].as_array().is_some_and(|p| p.len() >= peers)
            {
                return value;
            }
        }
        assert!(
            Instant::now() < end,
            "runtime readiness timeout; see {}",
            node.root.display()
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let binary = Path::new(&args[1]);
    let root = Path::new(&args[2]);
    assert!(!root.exists(), "probe needs a fresh root");
    std::fs::create_dir_all(root).unwrap();
    let mut owners = Vec::new();
    let mut settings = Vec::new();
    let mut locks = Vec::new();
    for i in 0..3 {
        let path = root.join(i.to_string());
        locks.push(identity::lock_profile(&path).unwrap());
        owners.push(identity::ensure(&path).unwrap());
        settings.push(Settings {
            connection: Connection::Private { invite: None },
            console_port: 33442 + i * 2,
            api_port: 33443 + i * 2,
            ..Default::default()
        });
    }
    let mut a = start(binary, &root.join("0"), settings[0].clone());
    let state = wait(&mut a, &owners[0].owner_id(), 0);
    assert_eq!(state["version"], "0.76.1");
    let offer = invitation::create(
        &owners[0],
        &settings[0],
        state["token"].as_str().unwrap(),
        now(),
    )
    .unwrap();
    settings[0] = invitation::remember_invitation(&settings[0], &offer, now()).unwrap();
    settings[1] = invitation::accept(&owners[1], &settings[1], &offer, now()).unwrap();
    settings[0] = invitation::decide_acceptance(
        &owners[0],
        &settings[0],
        settings[1].membership_receipt.as_ref().unwrap(),
        true,
        now(),
    )
    .unwrap();
    settings[1] = invitation::apply_receipt(
        &owners[1].owner_id(),
        &settings[1],
        settings[0].membership_receipt.as_ref().unwrap(),
        now(),
    )
    .unwrap();
    drop(a);
    let mut a = start(binary, &root.join("0"), settings[0].clone());
    wait(&mut a, &owners[0].owner_id(), 0);
    let mut b = start(binary, &root.join("1"), settings[1].clone());
    let state = wait(&mut b, &owners[1].owner_id(), 1);
    wait(&mut a, &owners[0].owner_id(), 1);
    println!(
        "PASS A invites B: both official serving nodes have a peer and verified distinct owners"
    );
    let onward = invitation::create(
        &owners[1],
        &settings[1],
        state["token"].as_str().unwrap(),
        now(),
    )
    .unwrap();
    settings[1] = invitation::remember_invitation(&settings[1], &onward, now()).unwrap();
    settings[2] = invitation::accept(&owners[2], &settings[2], &onward, now()).unwrap();
    settings[1] = invitation::decide_acceptance(
        &owners[1],
        &settings[1],
        settings[2].membership_receipt.as_ref().unwrap(),
        true,
        now(),
    )
    .unwrap();
    for i in [0, 2] {
        settings[i] = invitation::apply_receipt(
            &owners[i].owner_id(),
            &settings[i],
            settings[1].membership_receipt.as_ref().unwrap(),
            now(),
        )
        .unwrap();
    }
    drop(a);
    drop(b);
    let mut a = start(binary, &root.join("0"), settings[0].clone());
    wait(&mut a, &owners[0].owner_id(), 0);
    let mut b = start(binary, &root.join("1"), settings[1].clone());
    wait(&mut b, &owners[1].owner_id(), 1);
    let mut c = start(binary, &root.join("2"), settings[2].clone());
    for (i, node) in [&mut a, &mut b, &mut c].into_iter().enumerate() {
        let value = wait(node, &owners[i].owner_id(), 2);
        std::fs::write(
            root.join(format!("status-{i}.json")),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
    println!("PASS B invites C: all three official serving nodes see two peers, no pairwise Admit");
    println!("No model inference claimed: this bounded probe validates membership and full-node networking only");
}

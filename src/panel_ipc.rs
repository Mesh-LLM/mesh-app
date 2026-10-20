//! Tray <-> panel: the panel drops one-line action files into
//! `panel-commands/`; the tray runs them as if clicked and publishes what the
//! panel shows in `panel-state.json`. Plain files keep both sides simple.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct State {
    pub status: String,
    pub sharing: bool,
    pub busy: bool,
    pub share_status: String,
    /// Friendly name of the model being served, if any.
    pub serving: Option<String>,
    /// Engine model id being served (for the agent setup prompt).
    pub model_id: Option<String>,
    pub public: bool,
    pub balance_sats: Option<u64>,
    pub wallet: bool,
    /// USD cents per BTC for the "≈ $" hints; `None` until a rate is known.
    pub usd_per_btc_cents: Option<u64>,
    /// Live inference counters for the Activity card.
    pub activity: Option<crate::status::Activity>,
    /// The tray's pid; the panel quits when it is gone.
    pub tray_pid: u32,
    /// Bumped by Settings while the panel is open: bring the window forward.
    pub raise: u64,
}

fn commands(root: &Path) -> PathBuf {
    root.join("panel-commands")
}

fn state_path(root: &Path) -> PathBuf {
    root.join("panel-state.json")
}

pub fn send(root: &Path, id: &str) -> Result<(), String> {
    let dir = commands(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{nanos}"));
    std::fs::write(&tmp, id).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(format!("{nanos:032}"))).map_err(|e| e.to_string())
}

/// Pending actions, oldest first; each is removed as it is read.
pub fn take_commands(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(commands(root)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let id = std::fs::read_to_string(&p).ok();
            let _ = std::fs::remove_file(&p);
            id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        })
        .collect()
}

pub fn write_state(root: &Path, state: &State) -> Result<(), String> {
    let text = serde_json::to_string(state).map_err(|e| e.to_string())?;
    let tmp = state_path(root).with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, state_path(root)).map_err(|e| e.to_string())
}

pub fn read_state(root: &Path) -> Option<State> {
    serde_json::from_str(&std::fs::read_to_string(state_path(root)).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_arrive_once_in_order_and_state_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        send(dir.path(), "public").unwrap();
        send(dir.path(), "compute").unwrap();
        assert_eq!(take_commands(dir.path()), ["public", "compute"]);
        assert!(take_commands(dir.path()).is_empty());
        let s = State {
            status: "Sharing".into(),
            sharing: true,
            ..Default::default()
        };
        write_state(dir.path(), &s).unwrap();
        assert_eq!(read_state(dir.path()), Some(s));
    }
}

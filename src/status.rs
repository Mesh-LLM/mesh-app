//! Read-only, bounded loopback requests. No peer-provided management URLs.
use std::time::Duration;

#[derive(Clone, Default)]
pub struct Snapshot {
    pub running: bool,
    pub models_available: bool,
    pub pid: Option<u32>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .redirects(0)
        .build()
}

fn get(port: u16, path: &str) -> Result<serde_json::Value, String> {
    let response = agent()
        .get(&format!("http://127.0.0.1:{port}{path}"))
        .call()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    use std::io::Read;
    response
        .into_reader()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Mesh response too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

pub fn snapshot(port: u16) -> Snapshot {
    let Ok(value) = get(port, "/api/status") else {
        return Snapshot::default();
    };
    if !value["peers"].is_array() {
        return Snapshot::default();
    }
    let models_available = get(port, "/v1/models").is_ok_and(|v| actual_models(&v));

    let pid = value["local_instances"]
        .as_array()
        .and_then(|instances| {
            instances
                .iter()
                .find(|instance| instance["is_self"].as_bool() == Some(true))
        })
        .and_then(|instance| instance["pid"].as_u64())
        .and_then(|pid| u32::try_from(pid).ok());
    Snapshot {
        pid,
        running: true,
        models_available,
    }
}

fn actual_models(value: &serde_json::Value) -> bool {
    value["data"].as_array().is_some_and(|models| {
        models.iter().any(|model| {
            model["id"]
                .as_str()
                .is_some_and(|id| !id.is_empty() && !matches!(id, "auto" | "mesh"))
        })
    })
}

#[cfg(windows)]
pub fn stop_owned(port: u16, pid: u32) -> Result<(), String> {
    let status = get(port, "/api/status")?;
    // Local instance metadata must identify this retained child, not merely a listener.
    let owned = status["local_instances"]
        .as_array()
        .is_some_and(|instances| {
            instances.iter().any(|instance| {
                instance["pid"].as_u64() == Some(u64::from(pid))
                    && instance["is_self"].as_bool() == Some(true)
            })
        });
    if !owned {
        return Err("Cannot verify Mesh process ownership; leaving it running".into());
    }
    agent()
        .post(&format!("http://127.0.0.1:{port}/api/runtime/shutdown"))
        .send_string("")
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_routes_are_not_readiness() {
        assert!(!actual_models(
            &serde_json::json!({"data":[{"id":"auto"},{"id":"mesh"}]})
        ));
        assert!(!actual_models(&serde_json::json!({"data":[]})));
        assert!(actual_models(
            &serde_json::json!({"data":[{"id":"real/model"}]})
        ));
    }
    #[test]
    fn unavailable_daemon_is_not_ready() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!snapshot(port).running);
    }
}

use serde::Deserialize;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct PairingSession {
    pub id: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub peer_name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub comparison_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Sessions {
    #[serde(default)]
    sessions: Vec<PairingSession>,
}

/// One nearby/published mesh from `GET /api/discover`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredMesh {
    pub label: String,
    pub invite_token: String,
}

#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    pub running: bool,
    pub peers: Vec<String>,
    pub pairing_supported: bool,
    pub pending: Vec<PairingSession>,
    /// Models this node could load, from `/api/status.available_models`.
    pub available_models: Vec<String>,
    /// Models actually being served right now.
    pub serving_models: Vec<String>,
    /// Owner-control endpoint token, required by `/api/runtime/control/*`.
    pub control_endpoint: Option<String>,
    pub discovery_mode: String,
}

pub fn snapshot(port: u16) -> Snapshot {
    let mut snap = Snapshot::default();
    let Ok(status) = request(port, "GET", "/api/status", "") else {
        return snap;
    };
    snap.running = true;
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&status) {
        if let Some(peers) = value.get("peers").and_then(|p| p.as_array()) {
            snap.peers = peers.iter().map(peer_label).collect();
        }
        snap.available_models = string_list(&value, "available_models");
        snap.serving_models = string_list(&value, "serving_models");
        snap.discovery_mode = value
            .get("mesh_discovery_mode")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
    }
    match request(port, "GET", "/api/pairing/sessions", "") {
        Ok(body) => {
            snap.pairing_supported = true;
            if let Ok(parsed) = serde_json::from_str::<Sessions>(&body) {
                snap.pending = parsed
                    .sessions
                    .into_iter()
                    .filter(|s| s.status == "awaiting_approval" && s.direction == "incoming")
                    .collect();
            }
        }
        Err(_) => snap.pairing_supported = false,
    }
    snap.control_endpoint = control_endpoint(port);
    snap
}

fn string_list(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn peer_label(peer: &serde_json::Value) -> String {
    // `/api/status` PeerPayload has `hostname` (optional) and `id`;
    // latency is `latency_ms` when measured, else `rtt_ms`.
    let name = peer
        .get("hostname")
        .and_then(|v| v.as_str())
        .or_else(|| peer.get("id").and_then(|v| v.as_str()))
        .unwrap_or("peer");
    match peer
        .get("latency_ms")
        .or_else(|| peer.get("rtt_ms"))
        .and_then(|v| v.as_f64())
    {
        Some(ms) => format!("{name} · {ms:.0} ms"),
        None => name.to_string(),
    }
}

/// `GET /api/discover`. Blocks for seconds (mDNS browse / relay query), so this
/// is called on demand from the menu, not from the status poll.
pub fn discover(port: u16) -> Vec<DiscoveredMesh> {
    let Ok(body) = request_with_timeout(port, "GET", "/api/discover", "", Duration::from_secs(15))
    else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Vec::new();
    };
    value
        .as_array()
        .map(|entries| entries.iter().filter_map(discovered_mesh).collect())
        .unwrap_or_default()
}

fn discovered_mesh(entry: &serde_json::Value) -> Option<DiscoveredMesh> {
    let listing = entry.get("listing").unwrap_or(entry);
    let invite_token = listing.get("invite_token")?.as_str()?.to_string();
    let nodes = listing
        .get("node_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let vram_gb = listing
        .get("total_vram_bytes")
        .and_then(|v| v.as_f64())
        .map(|bytes| bytes / 1e9)
        .unwrap_or(0.0);
    let name = listing
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            listing
                .get("mesh_id")
                .and_then(|v| v.as_str())
                .map(|id| format!("mesh {}", &id[..id.len().min(8)]))
        })
        .unwrap_or_else(|| "mesh".to_string());
    Some(DiscoveredMesh {
        label: format!("{name} · {nodes} nodes · {vram_gb:.0} GB"),
        invite_token,
    })
}

/// The owner-control endpoint token. `/api/runtime/control/*` refuses to infer
/// it ("owner-control endpoint must be supplied explicitly").
pub fn control_endpoint(port: u16) -> Option<String> {
    let body = request(port, "GET", "/api/runtime/control-bootstrap", "").ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&body).ok()?;
    value
        .get("endpoint")?
        .as_str()
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

pub fn load_model(port: u16, endpoint: &str, model: &str) -> Result<(), String> {
    let body = serde_json::json!({ "model": model, "endpoint": endpoint }).to_string();
    request_with_timeout(
        port,
        "POST",
        "/api/runtime/control/load-model",
        &body,
        Duration::from_secs(120),
    )
    .map(|_| ())
}

pub fn unload_model(port: u16, endpoint: &str, model: &str) -> Result<(), String> {
    let body = serde_json::json!({ "model": model, "endpoint": endpoint }).to_string();
    request_with_timeout(
        port,
        "POST",
        "/api/runtime/control/unload-model",
        &body,
        Duration::from_secs(60),
    )
    .map(|_| ())
}

pub fn decide(port: u16, id: &str, decision: &str) -> Result<(), String> {
    request(
        port,
        "POST",
        &format!("/api/pairing/sessions/{id}/{decision}"),
        "",
    )
    .map(|_| ())
}

/// Graceful shutdown. Only present on newer daemons — released 0.76.0-rc9
/// returns 404, in which case the caller falls back to signalling the child.
pub fn shutdown(port: u16) -> Result<(), String> {
    request(port, "POST", "/api/runtime/shutdown", "").map(|_| ())
}

fn request(port: u16, method: &str, path: &str, body: &str) -> Result<String, String> {
    request_with_timeout(port, method, path, body, Duration::from_secs(3))
}

fn request_with_timeout(
    port: u16,
    method: &str,
    path: &str,
    body: &str,
    timeout: Duration,
) -> Result<String, String> {
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(600))
        .map_err(|e| format!("mesh not reachable on {port}: {e}"))?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "incomplete response".to_string())?;
    let status = head.lines().next().unwrap_or_default();
    if status.contains(" 200 ") || status.contains(" 201 ") || status.contains(" 202 ") {
        Ok(body.to_string())
    } else {
        Err(format!("mesh rejected request: {status} {body}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pending_incoming_sessions() {
        let parsed: Sessions = serde_json::from_str(
            r#"{"sessions":[{"id":"a","direction":"incoming","peer_name":"Kitchen PC","status":"awaiting_approval","comparison_code":"418302"},{"id":"b","direction":"outgoing","peer_name":"X","status":"awaiting_approval"}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.sessions.len(), 2);
    }

    #[test]
    fn peer_label_uses_latency_when_present() {
        let peer = serde_json::json!({"hostname":"Kitchen PC","id":"abc","latency_ms":12});
        assert_eq!(peer_label(&peer), "Kitchen PC · 12 ms");
        let rtt_only = serde_json::json!({"hostname":"Dad's PC","rtt_ms":210});
        assert_eq!(peer_label(&rtt_only), "Dad's PC · 210 ms");
        let bare = serde_json::json!({"id":"abc"});
        assert_eq!(peer_label(&bare), "abc");
    }

    /// Shape taken from a live `GET /api/discover` on 0.76.0-rc9: a top-level
    /// array whose entries wrap the mesh in a `listing` object.
    #[test]
    fn parses_live_discover_entry_shape() {
        let entry = serde_json::json!({
            "listing": {
                "invite_token": "eyJhIjoxfQ",
                "node_count": 6,
                "total_vram_bytes": 76700188672u64,
                "mesh_id": "e7732132665a3206ab29ea8e56ce4470"
            },
            "publisher_npub": "npub1x0"
        });
        let mesh = discovered_mesh(&entry).expect("parsed");
        assert_eq!(mesh.invite_token, "eyJhIjoxfQ");
        assert_eq!(mesh.label, "mesh e7732132 · 6 nodes · 77 GB");
    }

    #[test]
    fn discover_entry_without_token_is_skipped() {
        let entry = serde_json::json!({ "listing": { "node_count": 2 } });
        assert!(discovered_mesh(&entry).is_none());
    }
}

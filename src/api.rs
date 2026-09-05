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

#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    pub running: bool,
    pub peers: Vec<String>,
    pub pairing_supported: bool,
    pub pending: Vec<PairingSession>,
}

pub fn snapshot(port: u16) -> Snapshot {
    let mut snap = Snapshot::default();
    let Ok(status) = request(port, "GET", "/api/status") else {
        return snap;
    };
    snap.running = true;
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&status) {
        if let Some(peers) = value.get("peers").and_then(|p| p.as_array()) {
            snap.peers = peers.iter().map(peer_label).collect();
        }
    }
    match request(port, "GET", "/api/pairing/sessions") {
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
    snap
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

pub fn decide(port: u16, id: &str, decision: &str) -> Result<(), String> {
    request(
        port,
        "POST",
        &format!("/api/pairing/sessions/{id}/{decision}"),
    )
    .map(|_| ())
}

pub fn shutdown(port: u16) -> Result<(), String> {
    request(port, "POST", "/api/runtime/shutdown").map(|_| ())
}

fn request(port: u16, method: &str, path: &str) -> Result<String, String> {
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(600))
        .map_err(|e| format!("mesh not reachable on {port}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
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
        Err(format!("mesh rejected request: {status}"))
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
}

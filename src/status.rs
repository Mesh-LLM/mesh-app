//! Read-only, bounded loopback requests. No peer-provided management URLs.
use std::time::Duration;

#[derive(Clone, Default)]
pub struct Snapshot {
    pub running: bool,
    pub models_available: bool,
    pub pid: Option<u32>,
    pub private_owner: Option<String>,
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
        private_owner: value["owner"]["owner_id"]
            .as_str()
            .filter(|_| {
                value["owner"]["verified"].as_bool() == Some(true)
                    && value["publication_state"].as_str() == Some("private")
                    && matches!(
                        value["runtime"]["daemon_state"].as_str(),
                        Some("ready_idle" | "ready_proxying" | "ready_serving")
                    )
            })
            .map(String::from),
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

/// A fresh read from the retained child, not the pre-restart status snapshot.
pub fn private_invite(port: u16, pid: u32, owner: &str) -> Result<String, String> {
    let value = get(port, "/api/status")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    checked_private_invite(&value, pid, owner, now)
}
fn checked_private_invite(
    value: &serde_json::Value,
    pid: u32,
    owner: &str,
    now: u128,
) -> Result<String, String> {
    let owned = value["local_instances"]
        .as_array()
        .is_some_and(|instances| {
            instances.iter().any(|i| {
                i["is_self"].as_bool() == Some(true) && i["pid"].as_u64() == Some(u64::from(pid))
            })
        });
    if !owned
        || value["publication_state"].as_str() != Some("private")
        || value["nostr_discovery"].as_bool() != Some(false)
        || value["owner"]["owner_id"].as_str() != Some(owner)
        || value["owner"]["verified"].as_bool() != Some(true)
        || value["owner"]["status"].as_str() != Some("verified")
        || !value["owner"]["cert_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
        || !matches!(
            value["runtime"]["daemon_state"].as_str(),
            Some("ready_idle" | "ready_proxying" | "ready_serving")
        )
        || !value["owner"]["expires_at_unix_ms"]
            .as_u64()
            .is_some_and(|expiry| u128::from(expiry) > now)
    {
        return Err("Private Mesh is not ready with this profile's verified identity. Retry startup, then share the reply.".into());
    }
    let token = value["token"]
        .as_str()
        .ok_or("Mesh has not supplied an invitation yet.")?;
    crate::settings::validate_invite(token)?;
    Ok(token.into())
}
#[cfg(test)]
mod private_tests {
    use super::*;
    #[test]
    fn reply_requires_fresh_owned_private_identity() {
        let v = serde_json::json!({"local_instances":[{"is_self":true,"pid":42}],"publication_state":"private","nostr_discovery":false,"owner":{"owner_id":"owner","verified":true,"status":"verified","expires_at_unix_ms":999,"cert_id":"certificate"},"token":"complete-token","runtime":{"daemon_state":"ready_idle"}});
        assert_eq!(
            checked_private_invite(&v, 42, "owner", 100).unwrap(),
            "complete-token"
        );
        assert!(checked_private_invite(&v, 43, "owner", 100).is_err());
        assert!(checked_private_invite(&v, 42, "other", 100).is_err());
        assert!(checked_private_invite(&v, 42, "owner", 999).is_err());
        for (field, bad) in [
            ("publication_state", serde_json::json!("public")),
            ("nostr_discovery", serde_json::json!(true)),
            ("token", serde_json::json!("bad token")),
        ] {
            let mut v = v.clone();
            v[field] = bad;
            assert!(checked_private_invite(&v, 42, "owner", 100).is_err());
        }
        let mut not_ready = v.clone();
        not_ready["runtime"]["daemon_state"] = "starting".into();
        assert!(checked_private_invite(&not_ready, 42, "owner", 100).is_err());
        not_ready = v.clone();
        not_ready["owner"]["cert_id"] = "".into();
        assert!(checked_private_invite(&not_ready, 42, "owner", 100).is_err());
        let mut v = v;
        v["owner"]["verified"] = false.into();
        assert!(checked_private_invite(&v, 42, "owner", 100).is_err());
    }
}

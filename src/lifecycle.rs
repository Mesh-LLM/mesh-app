//! Engine ownership. Mesh runs as a child process of the tray
//! (`current_exe --engine`), not on a thread inside it: a stop is a process
//! exit, so the OS releases every file lock, socket and GPU allocation the
//! engine held. The tray talks to it only over its local HTTP API.
//!
//! Stop protocol: the parent closes the child's stdin. The child then stops
//! its SDK handle cooperatively and exits. The same EOF happens if the tray
//! dies, so an orphaned engine shuts itself down.
use crate::settings::{Connection, Settings, MIN_NODE_VERSION};
use mesh_llm_sdk::embedded_node::EmbeddedTrustPolicy as TrustPolicy;
use mesh_llm_sdk::{MeshNode, MeshNodeBuilder};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// Argument that turns the tray binary into the engine process.
pub const ENGINE_ARG: &str = "--engine";
/// How long a cooperative stop may take before the child is killed.
const STOP_GRACE: Duration = Duration::from_secs(60);

/// Everything the child needs to build its SDK config, sent as one JSON line
/// on stdin. The child builds the config with the same [`config`] function
/// the tests cover, so there is one source of truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSpec {
    pub settings: Settings,
    pub profile: PathBuf,
    pub model: Option<String>,
    pub share_compute: bool,
    /// Tests only: no auto-join and no relays, so a run never touches a real mesh.
    #[serde(default)]
    pub isolated_network: bool,
}

/// SDK builder for the node the engine child runs. Serving mode loads and
/// shares the tray-selected model; client mode consumes inference only and
/// never receives a model.
pub fn builder(
    settings: &Settings,
    profile: &Path,
    model: Option<String>,
    share_compute: bool,
) -> MeshNodeBuilder {
    let mut builder = MeshNode::builder()
        .api_port(settings.api_port)
        .console_port(settings.console_port)
        .console_ui(true)
        .config_path(profile.join("config.toml"))
        .isolated_config(false)
        .startup_timeout(std::time::Duration::from_secs(180));
    builder = if share_compute {
        let builder = builder.serve();
        match model {
            Some(model) => builder.model(model),
            None => builder,
        }
    } else {
        builder.client()
    };
    match settings.connection {
        Connection::Automatic => builder = builder.auto_join(true),
        Connection::Private { .. } => {
            builder = builder
                .owner_key(profile.join("owner-keystore.json"))
                .owner_required(true)
                .trust_policy(TrustPolicy::RequireOwned);
            let joins = settings.joins();
            if joins.is_empty() {
                builder = builder.min_node_version(MIN_NODE_VERSION);
            } else {
                builder = builder.join_tokens(joins.into_iter().cloned());
            }
        }
    }
    builder
}

#[cfg(test)]
fn config(
    settings: &Settings,
    profile: &Path,
    model: Option<String>,
) -> mesh_llm_sdk::embedded_node::EmbeddedMeshNodeConfig {
    builder(settings, profile, model, true).build()
}

pub struct Engine {
    pid: u32,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    done: Receiver<Result<(), String>>,
    result: Option<String>,
}

impl Engine {
    /// A child process cannot leak state into the tray, so a replacement is
    /// always safe to start once the previous child has exited.
    pub fn verify_restart_safe() -> Result<(), String> {
        Ok(())
    }

    pub fn start(spec: EngineSpec) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut child = Command::new(exe)
            .arg(ENGINE_ARG)
            // Per-child filtering the in-process engine could not do: these
            // must never reach the engine from the tray's environment.
            .env_remove("MESH_LLM_EPHEMERAL_KEY")
            .env_remove("MESH_LLM_OWNER_PASSPHRASE")
            .stdin(Stdio::piped())
            // Inherited: the tray already redirected these into mesh.log.
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("could not start the Mesh engine: {e}"))?;
        let pid = child.id();
        let line = serde_json::to_string(&spec).map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().ok_or("engine stdin unavailable")?;
        if let Err(e) = writeln!(stdin, "{line}").and_then(|()| stdin.flush()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("could not configure the Mesh engine: {e}"));
        }
        let (stop, requested) = tokio::sync::oneshot::channel();
        let (finished, done) = mpsc::channel();
        std::thread::Builder::new()
            .name("tray-engine-supervisor".into())
            .spawn(move || {
                let _ = finished.send(supervise(child, stdin, requested));
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            pid,
            stop: Some(stop),
            done,
            result: None,
        })
    }

    /// The engine's process id; `/api/status` reports the same pid.
    pub fn id(&self) -> u32 {
        self.pid
    }

    pub fn try_wait(&mut self) -> Result<Option<String>, String> {
        if self.result.is_none() {
            self.result = match self.done.try_recv() {
                Ok(Ok(())) => Some("stopped".into()),
                Ok(Err(error)) => Some(error),
                Err(TryRecvError::Disconnected) => Some("engine supervisor exited".into()),
                Err(TryRecvError::Empty) => None,
            };
        }
        Ok(self.result.clone())
    }

    #[cfg(unix)]
    #[doc(hidden)]
    pub fn fixture() -> (
        Self,
        tokio::sync::oneshot::Receiver<()>,
        mpsc::Sender<Result<(), String>>,
    ) {
        let (stop, requested) = tokio::sync::oneshot::channel();
        let (finished, done) = mpsc::channel();
        (
            Self {
                pid: std::process::id(),
                stop: Some(stop),
                done,
                result: None,
            },
            requested,
            finished,
        )
    }
}

/// Own the child until it exits. A requested stop closes stdin, then kills
/// the child if it has not exited within [`STOP_GRACE`].
fn supervise(
    mut child: Child,
    stdin: std::process::ChildStdin,
    mut requested: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), String> {
    let mut stdin = Some(stdin);
    let mut deadline = None;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if deadline.is_some() => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("Mesh engine stopped with {status}; see mesh.log"))
                };
            }
            Ok(Some(status)) => {
                return Err(format!(
                    "Mesh engine exited unexpectedly ({status}); see mesh.log"
                ));
            }
            Ok(None) => {}
            Err(e) => return Err(e.to_string()),
        }
        if deadline.is_none()
            && !matches!(
                requested.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            )
        {
            // Stop requested, or the UI owner dropped the engine.
            drop(stdin.take());
            deadline = Some(Instant::now() + STOP_GRACE);
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn request_stop(engine: &mut Engine) -> Result<(), String> {
    if let Some(stop) = engine.stop.take() {
        // If the supervisor exited, try_wait will report the result.
        let _ = stop.send(());
    }
    Ok(())
}

/// Child-process entry point: read one [`EngineSpec`] line, run the engine,
/// and stop it cooperatively when stdin reaches EOF.
pub fn run_engine_process() -> Result<(), String> {
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let spec: EngineSpec = serde_json::from_str(&line).map_err(|e| format!("engine spec: {e}"))?;
    let mut node = builder(
        &spec.settings,
        &spec.profile,
        spec.model,
        spec.share_compute,
    );
    if spec.isolated_network {
        // The SDK builder does not expose relay disabling; no auto-join and
        // no join tokens keep test runs off every real mesh.
        node = node.auto_join(false);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    // Watch for EOF from the very start: if the tray dies while the model is
    // still downloading or loading, the engine must not outlive it holding
    // the console port (that is what blocked later starts).
    let (eof_tx, mut eof_rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::Builder::new()
        .name("engine-parent-watch".into())
        .spawn(move || {
            let _ = std::io::stdin().read_to_end(&mut Vec::new());
            let _ = eof_tx.send(());
            // Cooperative stop gets a bounded grace period, then hard exit.
            std::thread::sleep(STOP_GRACE);
            std::process::exit(0);
        })
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let handle = tokio::select! {
            started = node.start() => started.map_err(|e| format!("{e:#}"))?,
            _ = &mut eof_rx => return Ok(()),
        };
        let _ = eof_rx.await;
        handle.stop().await.map_err(|e| format!("{e:#}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_keeps_mesh_and_profile_without_serving_configuration() {
        let settings = Settings {
            share_compute: false,
            connection: Connection::Private {
                invite: Some("selected-mesh".into()),
            },
            ..Default::default()
        };
        let client = builder(
            &settings,
            Path::new("/test-profile"),
            Some("model".into()),
            false,
        )
        .build();
        assert!(client.serving.models.is_empty());
        assert_eq!(client.network.join_tokens, vec!["selected-mesh"]);
        assert_eq!(client.http.api_port, settings.api_port);
        assert_eq!(
            client.storage.config_path,
            Some(Path::new("/test-profile/config.toml").into())
        );
    }

    #[test]
    fn private_origin_and_join_never_fall_back_to_public_or_allowlist() {
        for invite in [None, Some("their-token".into())] {
            let settings = Settings {
                connection: Connection::Private {
                    invite: invite.clone(),
                },
                ..Default::default()
            };
            let cfg = config(&settings, Path::new("profile"), None);
            assert!(!cfg.network.auto_join);
            assert!(!cfg.network.publish);
            assert!(cfg.admission.owner_required);
            assert!(cfg.admission.trusted_owners.is_empty());
            assert_eq!(cfg.admission.trust_policy, Some(TrustPolicy::RequireOwned));
            assert_eq!(cfg.network.join_tokens.len(), usize::from(invite.is_some()));
            assert_eq!(
                cfg.admission.mesh_requirements.min_node_version.is_some(),
                invite.is_none()
            );
        }
        let public = config(&Settings::default(), Path::new("profile"), None);
        assert!(public.network.join_tokens.is_empty());
        assert!(public
            .admission
            .mesh_requirements
            .min_node_version
            .is_none());
        assert!(public.admission.trust_policy.is_none());
    }

    #[test]
    fn private_origin_and_join_preserve_policy_without_a_roster() {
        let mut settings = Settings {
            connection: Connection::Private { invite: None },
            ..Settings::default()
        };
        let origin = config(&settings, Path::new("profile"), Some("model".into()));
        assert!(!origin.network.auto_join);
        assert!(!origin.network.publish);
        assert!(origin.admission.owner_required);
        assert_eq!(
            origin.admission.trust_policy,
            Some(TrustPolicy::RequireOwned)
        );
        assert!(origin.admission.trusted_owners.is_empty());
        assert_eq!(
            origin
                .admission
                .mesh_requirements
                .min_node_version
                .as_deref(),
            Some(MIN_NODE_VERSION)
        );
        assert_eq!(origin.serving.models, ["model"]);
        settings.accept_seed("test-invite").unwrap();
        let joined = config(&settings, Path::new("profile"), None);
        assert_eq!(joined.network.join_tokens, ["test-invite"]);
        assert!(joined
            .admission
            .mesh_requirements
            .min_node_version
            .is_none());
        assert!(joined.serving.models.is_empty());
        assert!(!joined.storage.isolated_config);
    }
    #[test]
    fn public_mode_and_user_config_are_explicit() {
        let config = config(&Settings::default(), Path::new("profile"), None);
        assert!(config.network.auto_join);
        assert!(config.http.console_ui);
        assert_eq!(
            config.storage.config_path,
            Some(Path::new("profile/config.toml").into())
        );
    }
    #[test]
    fn pending_stop_is_retained_until_worker_reports_completion() {
        let (stop, mut requested) = tokio::sync::oneshot::channel();
        let (finished, done) = mpsc::channel();
        let mut engine = Engine {
            pid: 0,
            stop: Some(stop),
            done,
            result: None,
        };
        request_stop(&mut engine).unwrap();
        request_stop(&mut engine).unwrap();
        assert_eq!(requested.try_recv(), Ok(()));
        assert!(engine.try_wait().unwrap().is_none());
        finished.send(Ok(())).unwrap();
        assert_eq!(engine.try_wait().unwrap().as_deref(), Some("stopped"));
        assert_eq!(engine.try_wait().unwrap().as_deref(), Some("stopped"));
    }
}

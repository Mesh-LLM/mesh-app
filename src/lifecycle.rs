//! In-process engine ownership. A pending stop never cancels the SDK
//! startup future: the worker retains it, then stops the resulting handle before
//! reporting completion. No replacement may start until completion is observed.
use mesh_llm_sdk::{serve, TrustPolicy};
use mesh_tray::settings::{Connection, Settings, MIN_NODE_VERSION};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

// The tested engine revision can return a startup error after its shutdown wait times out,
// dropping (detaching) the native runtime thread. An error is not proof of exit.
// Conservatively forbid another start in this process after any worker failure.
static RESTART_UNSAFE: AtomicBool = AtomicBool::new(false);

fn check_restart(safety: &AtomicBool) -> Result<(), String> {
    if safety.load(Ordering::Acquire) {
        Err("Restart the Mesh app before retrying: the embedded SDK did not prove that its previous runtime exited. No replacement runtime was started.".into())
    } else {
        Ok(())
    }
}

fn record_completion(safety: &AtomicBool, result: &Result<(), String>) {
    if result.is_err() {
        safety.store(true, Ordering::Release);
    }
}

pub fn config(
    settings: &Settings,
    profile: &Path,
    model: Option<String>,
) -> serve::EmbeddedServeConfig {
    let mut builder = serve::EmbeddedServeConfig::builder()
        .api_port(settings.api_port)
        .console_port(settings.console_port)
        .console_ui(true)
        .config_path(profile.join("config.toml"))
        .isolated_config(false)
        .startup_timeout(std::time::Duration::from_secs(180));
    if let Some(model) = model {
        builder = builder.model(model);
    }
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
    builder.build()
}

pub struct Engine {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    done: Receiver<Result<(), String>>,
    result: Option<String>,
}

impl Engine {
    pub fn start(config: serve::EmbeddedServeConfig) -> Result<Self, String> {
        check_restart(&RESTART_UNSAFE)?;
        // Environment filtering can be per-child but not per embedded thread.
        // Fail closed rather than mutate the process environment after threads start.
        for name in ["MESH_LLM_EPHEMERAL_KEY", "MESH_LLM_OWNER_PASSPHRASE"] {
            if std::env::var_os(name).is_some() {
                return Err(format!(
                    "Unset {name} before launching this embedded prototype"
                ));
            }
        }
        let (stop, requested) = tokio::sync::oneshot::channel();
        let (finished, done) = mpsc::channel();
        std::thread::Builder::new()
            .name("tray-engine".into())
            .spawn(move || {
                let result = (|| {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    runtime.block_on(async {
                        let handle = serve::start(config).await.map_err(|e| format!("{e:#}"))?;
                        // Dropping the UI owner also requests cooperative shutdown.
                        let _ = requested.await;
                        handle.stop().await.map_err(|e| format!("{e:#}"))
                    })
                })();
                record_completion(&RESTART_UNSAFE, &result);
                let _ = finished.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stop: Some(stop),
            done,
            result: None,
        })
    }

    pub fn id(&self) -> u32 {
        std::process::id()
    }

    pub fn try_wait(&mut self) -> Result<Option<String>, String> {
        if self.result.is_none() {
            self.result = match self.done.try_recv() {
                Ok(Ok(())) => Some("stopped".into()),
                Ok(Err(error)) => Some(error),
                Err(TryRecvError::Disconnected) => {
                    RESTART_UNSAFE.store(true, Ordering::Release);
                    Some("embedded worker disconnected; restart the app before retrying".into())
                }
                Err(TryRecvError::Empty) => None,
            };
        }
        Ok(self.result.clone())
    }

    #[cfg(all(test, unix))]
    pub fn fixture() -> (
        Self,
        tokio::sync::oneshot::Receiver<()>,
        mpsc::Sender<Result<(), String>>,
    ) {
        let (stop, requested) = tokio::sync::oneshot::channel();
        let (finished, done) = mpsc::channel();
        (
            Self {
                stop: Some(stop),
                done,
                result: None,
            },
            requested,
            finished,
        )
    }
}

pub fn request_stop(engine: &mut Engine) -> Result<(), String> {
    if let Some(stop) = engine.stop.take() {
        // If the receiver exited, try_wait will report the worker's result.
        let _ = stop.send(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn worker_failure_blocks_replacement_even_after_later_success() {
        let safety = AtomicBool::new(false);
        assert!(check_restart(&safety).is_ok());
        record_completion(&safety, &Err("startup cleanup timed out".into()));
        assert!(check_restart(&safety)
            .unwrap_err()
            .contains("Restart the Mesh app"));
        record_completion(&safety, &Ok(()));
        assert!(check_restart(&safety).is_err());
    }

    #[test]
    fn proven_stop_permits_restart() {
        let safety = AtomicBool::new(false);
        record_completion(&safety, &Ok(()));
        assert!(check_restart(&safety).is_ok());
    }

    #[test]
    fn pending_stop_is_retained_until_worker_reports_completion() {
        let (stop, mut requested) = tokio::sync::oneshot::channel();
        let (finished, done) = mpsc::channel();
        let mut engine = Engine {
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

    /// Real embedded engine start/stop, isolated from the user's profile and
    /// from any network: temp config, no relays, no auto-join, no model.
    /// Ignored by default; CI runs it explicitly on every OS.
    #[test]
    #[ignore]
    fn embedded_engine_starts_answers_status_and_stops() {
        use std::time::{Duration, Instant};
        let free = || {
            std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port()
        };
        let (api, console) = (free(), free());
        let dir = tempfile::tempdir().unwrap();
        let config = serve::EmbeddedServeConfig::builder()
            .api_port(api)
            .console_port(console)
            .console_ui(true)
            .config_path(dir.path().join("config.toml"))
            .isolated_config(true)
            .auto_join(false)
            .publish(false)
            .disable_iroh_relays(true)
            .startup_timeout(Duration::from_secs(120))
            .build();
        // Embedded startup never downloads a native runtime; CI installs the
        // released one for this OS first, exactly as the error message asks.
        {
            use mesh_llm_sdk::native_runtime::*;
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(install_native_runtime(NativeRuntimeInstallOptions {
                    mesh_version: CURRENT_MESH_VERSION.to_string(),
                    skippy_abi_version: Some(current_skippy_abi_version()),
                    ..Default::default()
                }))
                .expect("install native runtime");
        }
        let mut engine = Engine::start(config).expect("engine thread");
        let deadline = Instant::now() + Duration::from_secs(150);
        loop {
            if let Some(result) = engine.try_wait().unwrap() {
                panic!("engine exited before answering status: {result}");
            }
            let snapshot = crate::status::snapshot(console);
            if snapshot.running {
                assert_eq!(
                    snapshot.pid,
                    Some(std::process::id()),
                    "status is from this process"
                );
                break;
            }
            assert!(Instant::now() < deadline, "no /api/status within 150s");
            std::thread::sleep(Duration::from_millis(500));
        }
        request_stop(&mut engine).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let result = loop {
            if let Some(result) = engine.try_wait().unwrap() {
                break result;
            }
            assert!(Instant::now() < deadline, "engine did not stop within 60s");
            std::thread::sleep(Duration::from_millis(200));
        };
        assert_eq!(result, "stopped");
    }
}

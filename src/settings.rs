//! Launcher-owned preferences, not a second Mesh configuration model.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The node version floor Private declares when it creates a Mesh.
///
/// This is the one requirement the tray sets, and it is not really about
/// versions: a Mesh created with *any* requirement is requirement-aware, which
/// is what makes its invite a signed 24-hour bearer token that only the
/// originator can mint (`mesh-llm-host-runtime/src/mesh/node_identity.rs:106-185`).
/// Without a requirement the Mesh is unrestricted and the invite degrades to an
/// unsigned address token with no expiry.
///
/// It is a deliberate constant, not the bundled version, because the Mesh ID is
/// the hash of the policy: changing this value creates a *different* Mesh, and
/// the engine refuses to start Private against a genesis policy whose
/// requirements no longer match the flags
/// (`mesh/node_requirements.rs:129-135`). So bumping it means everybody
/// re-pastes a new invite, and that has to be a decision rather than a
/// side-effect of shipping a new runtime.
pub const MIN_NODE_VERSION: &str = "0.76.0";

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Connection {
    #[default]
    Automatic,
    Private {
        invite: Option<String>,
    },
}

/// Unknown keys are ignored on purpose: a `launcher.json` written by an earlier
/// tray carries allowlist-era grants and exchange state that this build has no
/// concept of, and refusing to open it would leave the user with an app that
/// cannot start. Those keys describe a Mesh whose membership model is gone, so
/// forgetting them is the correct migration, not a lossy one.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub connection: Connection,
    /// Legacy bootstrap addresses retained on ordinary restart. Explicit joining
    /// replaces this set rather than accumulating mesh selections.
    pub seeds: Vec<String>,
    pub console_port: u16,
    pub api_port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            connection: Connection::Automatic,
            seeds: Vec::new(),
            console_port: 3232,
            api_port: 9447,
        }
    }
}

impl Settings {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("launcher.json");
        let settings: Self = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                format!(
                    "Cannot read {}: {e}. Existing choice left unchanged.",
                    path.display()
                )
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.to_string()),
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.console_port == 0 || self.api_port == 0 || self.console_port == self.api_port {
            return Err("Mesh needs two different, nonzero ports".into());
        }
        if let Connection::Private {
            invite: Some(invite),
        } = &self.connection
        {
            validate_invite(invite)?;
        }
        if self.seeds.len() > 32 {
            return Err("Too many saved Mesh invites".into());
        }
        for seed in &self.seeds {
            validate_invite(seed)?;
        }
        Ok(())
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        use std::io::Write;
        self.validate()?;
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(root.join("launcher.json"))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Select this private mesh, replacing all previously saved invites.
    pub fn accept_seed(&mut self, seed: &str) -> Result<(), String> {
        validate_invite(seed)?;
        let next = Self {
            connection: Connection::Private {
                invite: Some(seed.into()),
            },
            seeds: Vec::new(),
            console_port: self.console_port,
            api_port: self.api_port,
        };
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Every invite this node holds, in the order it accepted them.
    pub fn joins(&self) -> Vec<&String> {
        let Connection::Private { invite } = &self.connection else {
            return Vec::new();
        };
        invite
            .iter()
            .chain(
                self.seeds
                    .iter()
                    .filter(|seed| Some(*seed) != invite.as_ref()),
            )
            .collect()
    }

    pub fn args(&self) -> Vec<String> {
        let mut args = vec![
            "serve".into(),
            "--console".into(),
            self.console_port.to_string(),
            "--port".into(),
            self.api_port.to_string(),
            "--log-format".into(),
            "json".into(),
        ];
        match &self.connection {
            Connection::Automatic => args.push("--auto".into()),
            Connection::Private { .. } => {
                // `require-owned` is what makes this a Mesh rather than a star:
                // every peer carrying a valid owner attestation and the same
                // signed Mesh policy is trusted, with no per-person list
                // (`mesh/ownership.rs:355-416`). Membership is therefore the
                // Mesh itself, and the invite is the membership.
                args.extend([
                    "--owner-required".into(),
                    "--trust-policy".into(),
                    "require-owned".into(),
                ]);
                let joins = self.joins();
                if joins.is_empty() {
                    // Creating: declare the requirement, which is what buys the
                    // signed bearer invite. A joiner must not declare it — it
                    // inherits the policy from the token it pastes, and its own
                    // requirements would describe a different Mesh.
                    args.extend(["--min-node-version".into(), MIN_NODE_VERSION.into()]);
                } else {
                    for join in joins {
                        args.extend(["--join".into(), join.clone()]);
                    }
                }
            }
        }
        args
    }
}

pub fn home() -> Result<PathBuf, String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| "Cannot locate your home directory".into())
}

/// Launcher-owned state: `launcher.json` and `mesh.log`, and nothing else.
pub fn data_root() -> Result<PathBuf, String> {
    Ok(home()?.join(".mesh-app"))
}

/// The one Mesh profile on this machine. The tray runs the engine as the user,
/// against the same `~/.mesh-llm` the plain CLI and Buzz use, so there is one
/// node identity per machine however Mesh is started.
pub fn mesh_profile() -> Result<PathBuf, String> {
    Ok(home()?.join(".mesh-llm"))
}

pub fn binary() -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "mesh-llm.exe"
    } else {
        "mesh-llm"
    };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundled = exe
        .parent()
        .ok_or("Cannot locate Mesh installation")?
        .join(name);
    if bundled.is_file() {
        Ok(bundled)
    } else {
        Err("Mesh runtime is missing from this installation. Reinstall Mesh.".into())
    }
}

pub fn validate_invite(invite: &str) -> Result<(), String> {
    if invite.is_empty()
        || invite.len() > 32 * 1024
        || !invite
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("Paste the complete Mesh invite, without spaces or a command.".into());
    }
    Ok(())
}

/// Whether pasted text is worth prefilling a join field with. A signed invite is
/// a long base64url token — around 2,200 characters — so this is deliberately
/// not the same thing as valid: a short word from the clipboard is not an
/// invite, and putting it in the field would look like Mesh had found one.
pub fn looks_like_invite(text: &str) -> bool {
    let text = text.trim();
    text.len() > 256 && validate_invite(text).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn joining_replaces_old_invites_and_survives_restart() {
        let root = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            connection: Connection::Private {
                invite: Some("original".into()),
            },
            seeds: vec!["old-seed".into()],
            console_port: 4242,
            api_port: 4243,
        };
        settings.accept_seed("new-mesh").unwrap();
        settings.save(root.path()).unwrap();
        let settings = Settings::load(root.path()).unwrap();
        assert!(settings.seeds.is_empty());
        assert_eq!((settings.console_port, settings.api_port), (4242, 4243));
        let args = settings.args();
        assert_eq!(
            args.windows(2)
                .filter(|p| p[0] == "--join")
                .map(|p| p[1].as_str())
                .collect::<Vec<_>>(),
            ["new-mesh"]
        );
        assert!(!args.contains(&"--min-node-version".into()));
    }

    #[test]
    fn joining_from_public_selects_only_the_supplied_invite() {
        let mut settings = Settings {
            seeds: vec!["legacy-seed".into()],
            ..Default::default()
        };
        settings.accept_seed("new-mesh").unwrap();
        settings.accept_seed("new-mesh").unwrap();
        assert_eq!(
            settings.connection,
            Connection::Private {
                invite: Some("new-mesh".into()),
            }
        );
        assert!(settings.seeds.is_empty());
        assert_eq!(settings.joins(), vec![&"new-mesh".to_string()]);
    }

    #[test]
    fn invalid_join_preserves_the_existing_selection() {
        let mut settings = Settings {
            connection: Connection::Private {
                invite: Some("original".into()),
            },
            seeds: vec!["old-seed".into()],
            ..Default::default()
        };
        let before = serde_json::to_value(&settings).unwrap();
        assert!(settings.accept_seed("").is_err());
        assert_eq!(serde_json::to_value(&settings).unwrap(), before);
    }

    #[test]
    fn the_machine_keeps_one_identity_in_both_modes() {
        // Switching Public/Private must not change which node you are: the
        // profile is the user's, and mode is only a set of flags.
        assert_eq!(mesh_profile().unwrap(), home().unwrap().join(".mesh-llm"));
        assert_eq!(data_root().unwrap(), home().unwrap().join(".mesh-app"));
        assert_ne!(mesh_profile().unwrap(), data_root().unwrap());
    }

    /// The version floor is not a version preference: it is what makes the Mesh
    /// requirement-aware, so only the node that *creates* the Mesh may declare
    /// it. A joiner that also declared it would be describing a second Mesh.
    #[test]
    fn only_the_creator_declares_the_requirement_and_a_joiner_only_joins() {
        let creating = Settings {
            connection: Connection::Private { invite: None },
            ..Default::default()
        };
        let args = creating.args();
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--min-node-version", MIN_NODE_VERSION]));
        assert!(!args.contains(&"--join".into()));

        let joining = Settings {
            connection: Connection::Private {
                invite: Some("their-token".into()),
            },
            ..Default::default()
        };
        let args = joining.args();
        assert!(!args.contains(&"--min-node-version".into()));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--join", "their-token"]));

        // Public declares neither: it is not a Mesh of ours to create or join.
        let public = Settings::default().args();
        assert!(!public.contains(&"--min-node-version".into()));
        assert!(!public.contains(&"--join".into()));
    }

    #[test]
    fn saves_and_reloads_private_connection() {
        let root = tempfile::tempdir().unwrap();
        let settings = Settings {
            connection: Connection::Private {
                invite: Some("saved-invite".into()),
            },
            ..Default::default()
        };
        settings.save(root.path()).unwrap();
        let loaded = Settings::load(root.path()).unwrap();
        assert_eq!(loaded.connection, settings.connection);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(root.path().join("launcher.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o077,
                0
            );
        }
    }
    #[test]
    fn rejects_invite_commands_and_urls() {
        for value in [
            "",
            "mesh-llm --join token",
            "https://example.com",
            "token\n",
        ] {
            assert!(validate_invite(value).is_err());
        }
        validate_invite("A-token_of-the_right-shape").unwrap();
    }
    #[test]
    fn only_a_token_sized_clipboard_prefills_the_join_field() {
        assert!(!looks_like_invite(""));
        assert!(!looks_like_invite("join my mesh"));
        assert!(!looks_like_invite(&"a".repeat(256)));
        assert!(looks_like_invite(&format!("  {}  ", "a".repeat(2187))));
        // Long, but not an invite: spaces and punctuation are not base64url.
        assert!(!looks_like_invite(&"word ".repeat(200)));
    }
    #[test]
    fn default_is_real_auto_not_a_fallback_ladder() {
        let args = Settings::default().args();
        assert_eq!(args[0], "serve");
        assert!(args.contains(&"--auto".into()));
        assert!(!args.contains(&"--publish".into()));
        assert!(!args.contains(&"--mesh-discovery-mode".into()));
    }
    #[test]
    fn private_choice_never_falls_back_publicly() {
        for invite in [None, Some("opaque-invite".into())] {
            let settings = Settings {
                connection: Connection::Private {
                    invite: invite.clone(),
                },
                ..Settings::default()
            };
            let args = settings.args();
            assert!(!args.contains(&"--auto".into()));
            assert!(!args.contains(&"--publish".into()));
            assert_eq!(args.contains(&"--join".into()), invite.is_some());
            assert_eq!(args[0], "serve");
        }
    }
    /// Mutual trust is the whole point of the pivot: a Private node must never
    /// launch with a per-person allowlist, in either the creating or the
    /// joining shape.
    #[test]
    fn private_is_owner_required_and_mutually_trusting_never_an_allowlist() {
        for invite in [None, Some("their-token".to_string())] {
            let settings = Settings {
                connection: Connection::Private { invite },
                ..Default::default()
            };
            let args = settings.args();
            assert!(args.contains(&"--owner-required".into()));
            assert!(args
                .windows(2)
                .any(|pair| pair == ["--trust-policy", "require-owned"]));
            assert!(!args.contains(&"allowlist".into()));
            assert!(!args.contains(&"--trust-owner".into()));
        }
        assert!(!Settings::default()
            .args()
            .contains(&"--trust-policy".into()));
    }
    #[test]
    fn a_launcher_file_from_the_allowlist_tray_still_opens_and_forgets_its_grants() {
        // The old fields describe a membership model that no longer exists.
        // Opening must succeed; the grants must not come back in any form.
        let settings: Settings = serde_json::from_str(
            r#"{"connection":{"mode":"private","invite":"old-invite"},
                "admitted_owners":["abababababababababababababababababababababababababababababababab"],
                "owner_names":{"abababababababababababababababababababababababababababababababab":"Jo"},
                "exchange":{"generation":3},"replies":[],"membership_receipt":null,
                "issued_membership_invitations":["x"],"applied_membership_receipts":[]}"#,
        )
        .unwrap();
        settings.validate().unwrap();
        let args = settings.args();
        assert!(!args.contains(&"--trust-owner".into()));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--trust-policy", "require-owned"]));
        assert!(args.windows(2).any(|pair| pair == ["--join", "old-invite"]));
        let written = serde_json::to_string(&settings).unwrap();
        assert!(!written.contains("admitted_owners"));
        assert!(!written.contains("exchange"));
    }
    #[test]
    fn malformed_saved_choice_is_not_reinterpreted_as_auto() {
        assert!(serde_json::from_str::<Settings>(r#"{"connection":{"mode":"typo"}}"#).is_err());
        let settings = Settings {
            connection: Connection::Private {
                invite: Some("".into()),
            },
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
    }
    #[test]
    fn roundtrips_private_choice_and_checks_ports() {
        let mut settings = Settings {
            connection: Connection::Private {
                invite: Some("opaque".into()),
            },
            ..Settings::default()
        };
        let decoded: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(settings.connection, decoded.connection);
        settings.api_port = settings.console_port;
        assert!(settings.validate().is_err());
    }
}

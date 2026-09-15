//! Launcher-owned preferences, not a second Mesh configuration model.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Connection {
    #[default]
    Automatic,
    Private {
        invite: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub connection: Connection,
    /// Owner identities explicitly approved on this node; never imported from an invite.
    pub admitted_owners: Vec<String>,
    /// Additional accepted bootstrap seeds; existing private connections survive new joins.
    pub seeds: Vec<String>,
    pub membership_receipt: Option<Vec<u8>>,
    pub pending_membership_acceptance: Option<String>,
    pub issued_membership_invitations: Vec<String>,
    pub applied_membership_receipts: Vec<String>,
    pub owner_names: std::collections::BTreeMap<String, String>,
    pub exchange: crate::exchange::ExchangeState,
    pub replies: Vec<crate::consent::Reply>,
    pub console_port: u16,
    pub api_port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            connection: Connection::Automatic,
            admitted_owners: Vec::new(),
            seeds: Vec::new(),
            membership_receipt: None,
            pending_membership_acceptance: None,
            issued_membership_invitations: Vec::new(),
            applied_membership_receipts: Vec::new(),
            owner_names: Default::default(),
            replies: Vec::new(),
            exchange: Default::default(),
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
        if self
            .membership_receipt
            .as_ref()
            .is_some_and(|bytes| bytes.len() > crate::exchange::MAX_FILE_BYTES)
            || self.applied_membership_receipts.len() > 4096
            || self.issued_membership_invitations.len() > 32
        {
            return Err("Too much saved membership data".into());
        }
        if self.seeds.len() > 32 {
            return Err("Too many saved Mesh seeds".into());
        }
        for seed in &self.seeds {
            validate_invite(seed)?;
        }
        crate::admission::validate_owners(&self.admitted_owners)?;
        if self.replies.len() > 32 {
            return Err("Too many pending replies".into());
        }
        if self.owner_names.len() > 1024
            || self.owner_names.iter().any(|(id, name)| {
                !self.admitted_owners.contains(id)
                    || name.len() > 128
                    || name.chars().any(char::is_control)
            })
        {
            return Err("Invalid allowed-person labels".into());
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

    /// Accept another seed without discarding established serving participation.
    pub fn accept_seed(&mut self, seed: &str) -> Result<(), String> {
        validate_invite(seed)?;
        match &self.connection {
            Connection::Private {
                invite: Some(current),
            } if current != seed => {
                if !self.seeds.iter().any(|saved| saved == seed) {
                    self.seeds.push(seed.into());
                }
            }
            Connection::Private { invite: Some(_) } => {}
            _ => {
                self.connection = Connection::Private {
                    invite: Some(seed.into()),
                }
            }
        }
        self.validate()
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
            Connection::Private { invite } => {
                args.extend([
                    "--owner-required".into(),
                    "--trust-policy".into(),
                    "allowlist".into(),
                ]);
                // The people this tray admitted are declared on the command
                // line, exactly as Buzz declares its roster through the SDK.
                // The engine merges these with the machine's trust store in
                // memory and writes nothing back, so the tray never edits the
                // user's trusted owners; the effective allowlist is the union.
                for owner in &self.admitted_owners {
                    args.extend(["--trust-owner".into(), owner.clone()]);
                }
                if let Some(invite) = invite {
                    args.extend(["--join".into(), invite.clone()]);
                }
                for seed in &self.seeds {
                    if Some(seed) != invite.as_ref() {
                        args.extend(["--join".into(), seed.clone()]);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepting_more_members_preserves_seeds_serving_and_grants() {
        let root = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            connection: Connection::Private {
                invite: Some("original".into()),
            },
            admitted_owners: vec!["ab".repeat(32)],
            ..Default::default()
        };
        settings.accept_seed("second").unwrap();
        settings.accept_seed("third").unwrap();
        settings.accept_seed("second").unwrap();
        settings.save(root.path()).unwrap();
        let settings = Settings::load(root.path()).unwrap();
        assert_eq!(
            settings.connection,
            Connection::Private {
                invite: Some("original".into())
            }
        );
        assert_eq!(settings.seeds, ["second", "third"]);
        assert_eq!(settings.admitted_owners, ["ab".repeat(32)]);
        let args = settings.args();
        assert_eq!(args[0], "serve");
        assert_eq!(
            args.windows(2)
                .filter(|p| p[0] == "--join")
                .map(|p| p[1].as_str())
                .collect::<Vec<_>>(),
            ["original", "second", "third"]
        );
    }

    #[test]
    fn the_machine_keeps_one_identity_in_both_modes() {
        // Switching Public/Private must not change which node you are: the
        // profile is the user's, and mode is only a set of flags.
        assert_eq!(mesh_profile().unwrap(), home().unwrap().join(".mesh-llm"));
        assert_eq!(data_root().unwrap(), home().unwrap().join(".mesh-app"));
        assert_ne!(mesh_profile().unwrap(), data_root().unwrap());
    }

    #[test]
    fn admitted_people_are_declared_on_the_command_line_in_private_only() {
        let owner = "ab".repeat(32);
        let second = "cd".repeat(32);
        let settings = Settings {
            connection: Connection::Private { invite: None },
            admitted_owners: vec![owner.clone(), second.clone()],
            ..Default::default()
        };
        let args = settings.args();
        assert_eq!(
            args.windows(2)
                .filter(|pair| pair[0] == "--trust-owner")
                .map(|pair| pair[1].as_str())
                .collect::<Vec<_>>(),
            [owner.as_str(), second.as_str()]
        );
        // Public shares the machine with anyone, so it declares nobody.
        let public = Settings {
            admitted_owners: vec![owner],
            ..Default::default()
        };
        assert!(!public.args().contains(&"--trust-owner".into()));
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
    #[test]
    fn private_requires_identity_and_allowlist_even_when_empty() {
        let settings = Settings {
            connection: Connection::Private { invite: None },
            ..Default::default()
        };
        let args = settings.args();
        assert!(args.contains(&"--owner-required".into()));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--trust-policy", "allowlist"]));
        assert!(!Settings::default()
            .args()
            .contains(&"--trust-policy".into()));
    }
    #[test]
    fn roster_survives_restart_and_removal() {
        let root = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            connection: Connection::Private { invite: None },
            admitted_owners: vec!["ab".repeat(32)],
            ..Default::default()
        };
        settings.save(root.path()).unwrap();
        let loaded = Settings::load(root.path()).unwrap();
        assert_eq!(loaded.admitted_owners, settings.admitted_owners);
        settings.admitted_owners.clear();
        settings.save(root.path()).unwrap();
        assert!(Settings::load(root.path())
            .unwrap()
            .admitted_owners
            .is_empty());
    }
    #[test]
    fn legacy_settings_have_no_implicit_grants() {
        let settings: Settings =
            serde_json::from_str(r#"{"connection":{"mode":"private","invite":"old-invite"}}"#)
                .unwrap();
        assert!(settings.admitted_owners.is_empty());
        assert!(settings.args().contains(&"--owner-required".into()));
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

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
        let mut settings: Self = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                format!(
                    "Cannot read {}: {e}. Existing choice left unchanged.",
                    path.display()
                )
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.to_string()),
        };
        settings.console_port = port_override("MESH_LLM_CONSOLE_PORT", settings.console_port)?;
        settings.api_port = port_override("MESH_LLM_API_PORT", settings.api_port)?;
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

    pub fn runtime_home(&self, root: &Path) -> PathBuf {
        match self.connection {
            Connection::Automatic => root.join("public-home"),
            Connection::Private { .. } => root.join("home"),
        }
    }

    pub fn runtime_profile(&self, root: &Path) -> PathBuf {
        self.runtime_home(root).join(".mesh-llm")
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

fn port_override(key: &str, default: u16) -> Result<u16, String> {
    match std::env::var(key) {
        Ok(value) => value.parse().map_err(|_| format!("Invalid {key}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(e.to_string()),
    }
}

pub fn data_root() -> Result<PathBuf, String> {
    for key in ["MESH_APP_DATA_DIR", "MESH_TRAY_DATA_DIR"] {
        if let Some(path) = std::env::var_os(key) {
            return Ok(PathBuf::from(path));
        }
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or("Cannot locate the Mesh app data directory")?;
    let home = PathBuf::from(home);
    let root = home.join(".mesh-app");
    adopt_previous_profile(&home.join(".mesh-tray"), &root)?;
    Ok(root)
}

/// Move an earlier release's `~/.mesh-tray` profile to the current name.
///
/// Renaming the directory would otherwise silently orphan an identity and every
/// remembered friend. Only an untouched destination is filled, and only from a
/// real directory: if both exist the current one is authoritative and the old
/// one is left on disk for the user to inspect or delete.
fn adopt_previous_profile(previous: &Path, root: &Path) -> Result<(), String> {
    if root.exists() || !previous.is_dir() {
        return Ok(());
    }
    std::fs::rename(previous, root).map_err(|e| {
        format!(
            "Cannot move {} to {}: {e}. The old profile was left unchanged.",
            previous.display(),
            root.display()
        )
    })
}

pub fn binary() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("MESH_LLM_BIN") {
        return Ok(PathBuf::from(path));
    }
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
        Err("Mesh runtime is missing from this installation. Reinstall Mesh or set MESH_LLM_BIN for development.".into())
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
    fn switching_modes_keeps_private_identity_out_of_public_profile() {
        let root = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let public = settings.runtime_profile(root.path());
        settings.connection = Connection::Private { invite: None };
        let private = settings.runtime_profile(root.path());
        assert_eq!(private, root.path().join("home/.mesh-llm"));
        assert_ne!(public, private);
        settings.connection = Connection::Automatic;
        assert_eq!(settings.runtime_profile(root.path()), public);
        settings.connection = Connection::Private {
            invite: Some("invite".into()),
        };
        assert_eq!(settings.runtime_profile(root.path()), private);
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
    fn an_earlier_profile_directory_is_adopted_but_never_overwrites_the_current_one() {
        let base = tempfile::tempdir().unwrap();
        let previous = base.path().join(".mesh-tray");
        let root = base.path().join(".mesh-app");
        // Nothing to adopt: silent no-op.
        adopt_previous_profile(&previous, &root).unwrap();
        assert!(!root.exists());
        // The old profile's identity and friends come across whole.
        std::fs::create_dir_all(previous.join("home/.mesh-llm")).unwrap();
        std::fs::write(previous.join("launcher.json"), "{}").unwrap();
        adopt_previous_profile(&previous, &root).unwrap();
        assert!(root.join("home/.mesh-llm").is_dir());
        assert!(root.join("launcher.json").exists());
        assert!(!previous.exists());
        // Both present: the current profile wins and the old one is left alone.
        std::fs::create_dir_all(&previous).unwrap();
        std::fs::write(previous.join("launcher.json"), "old").unwrap();
        adopt_previous_profile(&previous, &root).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("launcher.json")).unwrap(),
            "{}"
        );
        assert!(previous.join("launcher.json").exists());
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

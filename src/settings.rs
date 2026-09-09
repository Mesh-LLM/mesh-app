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
    pub console_port: u16,
    pub api_port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            connection: Connection::Automatic,
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

    fn validate(&self) -> Result<(), String> {
        if self.console_port == 0 || self.api_port == 0 || self.console_port == self.api_port {
            return Err("Mesh needs two different, nonzero ports".into());
        }
        if let Connection::Private {
            invite: Some(invite),
        } = &self.connection
        {
            if invite.is_empty()
                || invite.len() > 32 * 1024
                || invite.chars().any(char::is_whitespace)
            {
                return Err(
                    "Saved private invitation is invalid; automatic mode was not selected".into(),
                );
            }
        }
        Ok(())
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
                if let Some(invite) = invite {
                    args.extend(["--join".into(), invite.clone()]);
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
    if let Some(path) = std::env::var_os("MESH_TRAY_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or("Cannot locate the Mesh app data directory")?;
    Ok(PathBuf::from(home).join(".mesh-tray"))
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

#[cfg(test)]
mod tests {
    use super::*;
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
        }
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

//! An isolated agent profile and a user-owned terminal session, not another Mesh node.
use std::path::{Path, PathBuf};

pub fn config() -> String {
    let mut text = String::from("GOOSE_PROVIDER: openai\nGOOSE_MODE: approve\nextensions:\n  developer:\n    name: developer\n    type: builtin\n    enabled: true\n    bundled: true\n    available_tools: [shell]\n  skills:\n    name: skills\n    type: platform\n    enabled: true\n    bundled: true\n");
    for name in [
        "analyze",
        "todo",
        "apps",
        "chatrecall",
        "extensionmanager",
        "scheduler",
        "summon",
        "summarize",
        "code_execution",
        "orchestrator",
        "tom",
    ] {
        text.push_str(&format!(
            "  {name}:\n    name: {name}\n    type: platform\n    enabled: false\n"
        ));
    }
    text
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn script(goose: &Path, profile: &Path, folder: &Path, port: u16, model: &str) -> String {
    format!(
        "#!/bin/sh\nset -eu\nunset BUZZ_MANAGED_AGENT BUZZ_MANAGED_AGENT_START_NONCE BUZZ_PRIVATE_KEY BUZZ_AUTH_TAG\ncd {}\nexport GOOSE_PATH_ROOT={}\nexport GOOSE_PROVIDER=openai\nexport OPENAI_HOST={}\nexport OPENAI_BASE_PATH=v1/chat/completions\nexport OPENAI_API_KEY=mesh\nexport GOOSE_MODEL={}\nexport GOOSE_MODE=approve\nprintf '%s\\n' 'Goose · Mesh — shell + skills; shell commands ask for approval.'\nexec {} session --provider openai --model {}\n",
        quote(&folder.to_string_lossy()), quote(&profile.to_string_lossy()),
        quote(&format!("http://127.0.0.1:{port}")), quote(model),
        quote(&goose.to_string_lossy()), quote(model)
    )
}

pub fn find_agent(name: &str) -> Option<PathBuf> {
    if !matches!(name, "goose" | "pi") {
        return None;
    }
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        directories.push(PathBuf::from(home).join(".local/bin"));
    }
    directories.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    directories
        .into_iter()
        .map(|p| p.join(name))
        .find(|p| executable(p))
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn pi_config(port: u16, model: &str) -> String {
    serde_json::json!({"providers": {"mesh": {
        "baseUrl": format!("http://127.0.0.1:{port}/v1"),
        "api": "openai-completions", "apiKey": "mesh",
        "models": [{"id": model}]
    }}})
    .to_string()
}

pub fn pi_script(pi: &Path, profile: &Path, folder: &Path, model: &str) -> String {
    format!("#!/bin/sh\nset -eu\nunset BUZZ_MANAGED_AGENT BUZZ_MANAGED_AGENT_START_NONCE BUZZ_PRIVATE_KEY BUZZ_AUTH_TAG\ncd {}\nexport PI_CODING_AGENT_DIR={}\nexport PATH={}:\"$PATH\"\nprintf '%s\\n' 'Pi · Mesh — bash + read; commands run without approval.'\nexec {} --provider mesh --model {} --tools bash,read --no-extensions --no-mcp --offline\n",
        quote(&folder.to_string_lossy()), quote(&profile.to_string_lossy()),
        quote(&pi.parent().unwrap_or(Path::new("/usr/bin")).to_string_lossy()),
        quote(&pi.to_string_lossy()), quote(model))
}

pub fn model(port: u16) -> Result<String, String> {
    let response = ureq::get(&format!("http://127.0.0.1:{port}/v1/models"))
        .timeout(std::time::Duration::from_secs(3))
        .call()
        .map_err(|_| {
            "Mesh API is not ready. Wait for Mesh to connect, then try again.".to_string()
        })?;
    let value: serde_json::Value =
        serde_json::from_reader(response.into_reader()).map_err(|e| e.to_string())?;
    value["data"]
        .as_array()
        .and_then(|models| models.iter().find_map(|m| m["id"].as_str()))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            "No models are available yet. Connect to a Mesh or enable sharing, then try again."
                .into()
        })
}

pub fn prepare(
    agent: &str,
    root: &Path,
    folder: &Path,
    port: u16,
    model: &str,
) -> Result<PathBuf, String> {
    let executable =
        find_agent(agent).ok_or_else(|| format!("Install {agent} first, then try again."))?;
    // Each launch owns its config; a second launch cannot change an existing session.
    let launch = tempfile::Builder::new()
        .prefix(&format!("{agent}-"))
        .tempdir_in(root)
        .map_err(|e| e.to_string())?;
    let profile = launch.path().join("profile");
    std::fs::create_dir_all(profile.join("config")).map_err(|e| e.to_string())?;
    let content = if agent == "pi" {
        std::fs::write(profile.join("models.json"), pi_config(port, model))
            .map_err(|e| e.to_string())?;
        pi_script(&executable, &profile, folder, model)
    } else {
        std::fs::write(profile.join("config/config.yaml"), config()).map_err(|e| e.to_string())?;
        script(&executable, &profile, folder, port, model)
    };
    let command = launch.path().join("Launch Agent.command");
    std::fs::write(&command, content).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let _ = launch.keep();
    Ok(command)
}

pub fn open_terminal(command: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .args(["-a", "Terminal"])
            .arg(command)
            .env_remove("BUZZ_MANAGED_AGENT")
            .env_remove("BUZZ_MANAGED_AGENT_START_NONCE")
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(
                "Could not open Terminal. Open the saved Launch Agent.command file manually."
                    .into(),
            )
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = command;
        Err("This trial launcher supports macOS. Use Copy setup prompt on this platform.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_arguments_are_literal_and_profile_is_isolated() {
        let text = script(
            Path::new("/bin/goose"),
            Path::new("/tmp/profile"),
            Path::new("/tmp/it's $(false)"),
            1234,
            "model'; exit 1; '",
        );
        assert!(text.contains("cd '/tmp/it'\\''s $(false)'"));
        assert!(text.contains("GOOSE_PATH_ROOT='/tmp/profile'"));
        assert!(text.contains("BUZZ_MANAGED_AGENT_START_NONCE"));
        assert!(text.contains("--model 'model'\\''; exit 1; '\\'''"));
        assert!(text.contains("127.0.0.1:1234"));
    }
    #[test]
    fn pi_uses_isolated_provider_and_literal_arguments() {
        let value: serde_json::Value = serde_json::from_str(&pi_config(9447, "a\"b")).unwrap();
        assert_eq!(value["providers"]["mesh"]["models"][0]["id"], "a\"b");
        assert_eq!(
            value["providers"]["mesh"]["baseUrl"],
            "http://127.0.0.1:9447/v1"
        );
        let script = pi_script(
            Path::new("/opt/homebrew/bin/pi"),
            Path::new("/tmp/pi profile"),
            Path::new("/tmp/folder"),
            "$(false)",
        );
        assert!(script.contains("PI_CODING_AGENT_DIR='/tmp/pi profile'"));
        assert!(script.contains("--model '$(false)'"));
        assert!(script.contains("--tools bash,read --no-extensions --no-mcp"));
        assert!(script.contains("export PATH='/opt/homebrew/bin':"));
    }

    #[cfg(unix)]
    #[test]
    fn discovery_rejects_nonexecutable_files_and_directories() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pi");
        std::fs::write(&path, "test").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!executable(&path));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(executable(&path));
        assert!(!executable(dir.path()));
        assert!(find_agent("unknown").is_none());
    }

    #[test]
    fn only_shell_and_skills_enabled() {
        let text = config();
        assert_eq!(text.matches("enabled: true").count(), 2);
        assert!(text.contains("available_tools: [shell]"));
        assert!(text.contains("GOOSE_MODE: approve"));
        assert!(text.contains(
            "extensionmanager:\n    name: extensionmanager\n    type: platform\n    enabled: false"
        ));
    }
}

//! An isolated Goose profile and a user-owned terminal session, not another Mesh node.
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

pub fn find_goose() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|p| p.join("goose")));
    }
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".local/bin/goose"));
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/goose"),
        PathBuf::from("/usr/local/bin/goose"),
    ]);
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| "Install the Goose CLI first, then try again. Copy setup prompt works with other agents.".into())
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

pub fn prepare(root: &Path, folder: &Path, port: u16, model: &str) -> Result<PathBuf, String> {
    let goose = find_goose()?;
    // Each launch owns its config; a second launch cannot change an existing session.
    let launch = tempfile::Builder::new()
        .prefix("goose-")
        .tempdir_in(root)
        .map_err(|e| e.to_string())?;
    let profile = launch.path().join("profile");
    std::fs::create_dir_all(profile.join("config")).map_err(|e| e.to_string())?;
    std::fs::write(profile.join("config/config.yaml"), config()).map_err(|e| e.to_string())?;
    let command = launch.path().join("Launch Goose.command");
    std::fs::write(&command, script(&goose, &profile, folder, port, model))
        .map_err(|e| e.to_string())?;
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
                "Could not open Terminal. Open the saved Launch Goose.command file manually."
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

//! "Use with your agent": text the panel copies so a person can point an
//! agent (goose, pi, anything OpenAI-compatible) at this machine's Mesh. The
//! prompt asks the agent to do the configuring itself, preserving what is
//! already there, rather than the app editing other tools' config files.

/// The local OpenAI-compatible base URL the engine listens on.
pub fn endpoint(api_port: u16) -> String {
    format!("http://localhost:{api_port}/v1")
}

/// A prompt to paste into an agent. `model` is the model this node serves,
/// when it serves one; otherwise the agent is told to list what is available.
pub fn prompt(api_port: u16, model: Option<&str>) -> String {
    let url = endpoint(api_port);
    let model_line = match model {
        Some(m) => format!(
            "Use the model `{m}` if `{url}/models` lists it; otherwise pick one from that list."
        ),
        None => format!("Pick a model from `{url}/models`."),
    };
    format!(
        "Set yourself up to use my local Mesh LLM as a model provider.\n\
\n\
- It is an OpenAI-compatible API at {url} (chat completions, streaming supported).\n\
- No API key is needed. If your config requires one, use any placeholder such as `mesh`.\n\
- {model_line}\n\
\n\
Please:\n\
1. Look at how your provider/model configuration works (for goose or pi, read your own config or docs first).\n\
2. Add Mesh as an extra OpenAI-compatible provider. Keep my existing providers and settings; don't overwrite or remove anything.\n\
3. Verify it by sending one short chat request through the new provider and showing me the reply.\n\
4. Tell me exactly what you changed and how to switch back."
    )
}

/// A one-line check that the endpoint answers.
pub fn curl_check(api_port: u16) -> String {
    format!("curl -s {}/models", endpoint(api_port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_names_the_endpoint_model_and_asks_to_preserve_config() {
        let p = prompt(9447, Some("Qwen3-8B"));
        assert!(p.contains("http://localhost:9447/v1"));
        assert!(p.contains("`Qwen3-8B`"));
        assert!(p.contains("Keep my existing providers"));
        assert!(p.contains("one short chat request"));
        let p = prompt(1234, None);
        assert!(p.contains("Pick a model from `http://localhost:1234/v1/models`"));
        assert_eq!(curl_check(9447), "curl -s http://localhost:9447/v1/models");
    }
}

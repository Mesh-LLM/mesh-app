//! Conservative launcher policy, not Mesh discovery or an inference-engine policy.
//! Joining preserves local serving participation.
use crate::settings::Connection;

// One model, deliberately not Buzz's ladder. The tray serves a chat window in a
// menu bar, so its pick has to start quickly, fit anywhere, and answer: erring
// small costs some quality, erring large costs the product on the machine that
// cannot run it. Gemma 4 E4B answered every prompt in testing where the Qwen
// picks spent their whole token budget reasoning.
//
// Anyone who wants more sets MESH_TRAY_MODEL, which is why there is no memory
// probe here: a hardware tier the user can override with one variable is a
// guess with a maintenance cost on every platform.
const DEFAULT_MODEL: &str = "unsloth/gemma-4-E4B-it-GGUF@main:Q4_K_M";

pub fn local_model(connection: &Connection) -> Result<Option<String>, String> {
    if !matches!(connection, Connection::Private { .. }) {
        return Ok(None);
    }
    // An explicit choice is honoured as-is: trying a different model should not
    // require a rebuild, and the default below is a default, not a policy.
    Ok(Some(
        std::env::var("MESH_TRAY_MODEL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_does_not_select_and_private_joins_keep_serving() {
        assert_eq!(local_model(&Connection::Automatic).unwrap(), None);
        assert_eq!(
            local_model(&Connection::Private { invite: None }),
            local_model(&Connection::Private {
                invite: Some("token".into())
            })
        );
    }

    #[test]
    fn private_default_is_the_small_model_on_every_machine() {
        // No hardware tier: the same default regardless of memory or platform.
        assert_eq!(
            local_model(&Connection::Private { invite: None }).unwrap(),
            Some(DEFAULT_MODEL.to_string())
        );
    }
}

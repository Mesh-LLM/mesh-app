//! Conservative launcher policy, not Mesh discovery or an inference-engine policy.
//! Joining preserves local serving participation.
use crate::settings::Connection;

// Two picks, not a ladder: the small one is the default everywhere, and the
// large one is chosen only where there is no doubt it fits. Buzz's catalog
// (`desktop/src-tauri/src/mesh_llm/catalog.rs`) steps up at 32 GB; the tray
// deliberately waits until 64 GiB, because a tray chat window is the one place
// a model that does not fit is unrecoverable -- the user has no other model to
// switch to. Erring small costs quality; erring large costs the product.
//
// Both are Qwen again. They were briefly replaced by Gemma because they spent
// their whole token budget reasoning and often returned no answer; the tray now
// turns thinking off for its own runtime child (`runtime_config`), which
// removes that failure at its cause.
const SMALL: &str = "unsloth/Qwen3.5-9B-GGUF@main:Q4_K_M";
const LARGE: &str = "unsloth/Qwen3.8-27B-GGUF@main:Q4_K_M";

pub fn local_model(connection: &Connection) -> Result<Option<String>, String> {
    if !matches!(connection, Connection::Private { .. }) {
        return Ok(None);
    }
    // An explicit choice is honoured as-is: trying a different model should not
    // require a rebuild, and the ladder below is a default, not a policy.
    if let Some(chosen) = std::env::var("MESH_TRAY_MODEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(chosen));
    }
    choose(memory_bytes()?).map(str::to_string).map(Some)
}

fn choose(bytes: u64) -> Result<&'static str, String> {
    // Classified on the machine's rated memory, not on what is free right now:
    // a busy desktop must not silently drop a tier. Buzz derives that rating by
    // rounding to the nearest advertised capacity; whole GiB agrees with it at
    // both boundaries for the sizes Macs ship, so this reads memory directly
    // rather than taking a dependency on the runtime's hardware crate.
    match bytes / (1024 * 1024 * 1024) {
        64.. => Ok(LARGE),
        16.. => Ok(SMALL),
        _ => Err("Private hosting needs at least 16 GiB memory. This device cannot select a local model automatically. Set MESH_TRAY_MODEL to choose one yourself.".into()),
    }
}

#[cfg(target_os = "macos")]
fn memory_bytes() -> Result<u64, String> {
    let mut bytes = 0u64;
    let mut length = std::mem::size_of::<u64>();
    // SAFETY: valid NUL-terminated name and correctly sized writable u64 buffer;
    // no new value is supplied, so this is a read-only sysctl.
    let result = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || length != std::mem::size_of::<u64>() {
        return Err("Cannot read local memory for private model selection".into());
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn memory_bytes() -> Result<u64, String> {
    let info = std::fs::read_to_string("/proc/meminfo").map_err(|e| e.to_string())?;
    info.lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|kib| kib.checked_mul(1024))
        .ok_or_else(|| "Cannot read local memory for private model selection".into())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn memory_bytes() -> Result<u64, String> {
    Err("Private model selection is not supported on this platform yet".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_is_the_default_and_large_needs_no_doubt() {
        let gib = 1024 * 1024 * 1024;
        for size in [0, 4, 8, 15] {
            assert!(choose(size * gib).is_err());
        }
        for size in [16, 18, 24, 32, 36, 48, 63] {
            assert_eq!(choose(size * gib).unwrap(), SMALL);
        }
        for size in [64, 96, 128, 512] {
            assert_eq!(choose(size * gib).unwrap(), LARGE);
        }
    }

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
}

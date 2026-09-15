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
// Gemma both sides: measured on the same prompt, the Qwen picks spent their
// whole token budget reasoning and often returned no answer, while both Gemma
// picks thought briefly and answered every time. The tray also turns thinking
// off for its runtime child (`runtime_config`), but that is insurance, not the
// reason these two are here.
//
// Small is the default; the large pick is for genuinely large machines only,
// well above Buzz's 32 GB catalog step. A tray chat window is the one place a
// model that does not fit is unrecoverable, so the tray waits until there is no
// doubt at all.
//
// Quant names are the ones the repos actually publish: the 26B ships only
// `UD-Q4_K_M`, with no plain `Q4_K_M` file.
const SMALL: &str = "unsloth/gemma-4-E4B-it-GGUF@main:Q4_K_M";
const LARGE: &str = "unsloth/gemma-4-26B-A4B-it-GGUF@main:UD-Q4_K_M";

pub fn local_model(connection: &Connection) -> Result<Option<String>, String> {
    if !matches!(connection, Connection::Private { .. }) {
        return Ok(None);
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
        128.. => Ok(LARGE),
        8.. => Ok(SMALL),
        _ => Err("Private hosting needs at least 8 GiB memory. This device cannot select a local model automatically.".into()),
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
    fn small_is_the_default_and_large_needs_a_large_machine() {
        let gib = 1024 * 1024 * 1024;
        for size in [0, 4, 7] {
            assert!(choose(size * gib).is_err());
        }
        // Every Mac we test on except the 128 GiB M5 stays on the small pick.
        for size in [8, 16, 24, 32, 48, 64, 96, 127] {
            assert_eq!(choose(size * gib).unwrap(), SMALL);
        }
        for size in [128, 192, 256, 512] {
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

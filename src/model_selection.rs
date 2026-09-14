//! Conservative launcher policy, not Mesh discovery or an inference-engine policy.
//! Joining preserves local serving participation.
use crate::settings::Connection;

const SMALL: &str = "Qwen/Qwen2.5-3B-Instruct-GGUF@main:q4_k_m";
const MEDIUM: &str = "unsloth/Qwen3.5-9B-GGUF@main:Q4_K_M";

pub fn local_model(connection: &Connection) -> Result<Option<&'static str>, String> {
    if !matches!(connection, Connection::Private { .. }) {
        return Ok(None);
    }
    choose(memory_bytes()?).map(Some)
}

fn choose(bytes: u64) -> Result<&'static str, String> {
    // Catalog weights: 2.1 GB / 5.8 GB. Leave substantial headroom for the OS,
    // other applications, KV cache and the vision projector. Cap at 9B even on
    // large machines: this launcher favors a usable desktop over maximum size.
    match bytes / (1024 * 1024 * 1024) {
        24.. => Ok(MEDIUM),
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
    fn conservative_memory_thresholds_and_cap() {
        let gib = 1024 * 1024 * 1024;
        for size in [0, 4, 7] {
            assert!(choose(size * gib).is_err());
        }
        for size in [8, 16, 23] {
            assert_eq!(choose(size * gib).unwrap(), SMALL);
        }
        for size in [24, 32, 128, 512] {
            assert_eq!(choose(size * gib).unwrap(), MEDIUM);
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

//! Conservative launcher policy, not Mesh discovery or an inference-engine policy.
//! Joining preserves local serving participation.
use crate::settings::Connection;

// Total installed system memory, not free memory or dedicated GPU VRAM.
// These are conservative tray recommendations, not runtime fit guarantees.
// Explicit startup models in config.toml bypass this selection.
const SMALL: &str = "unsloth/gemma-4-E4B-it-GGUF@main:Q4_K_M";
const MEDIUM: &str = "unsloth/gemma-4-26B-A4B-it-GGUF@main:UD-Q4_K_M";
const LARGE: &str = "unsloth/Qwen3.8-27B-GGUF:UD-Q4_K_M";

pub fn local_model(connection: &Connection) -> Result<Option<String>, String> {
    if !matches!(connection, Connection::Private { .. }) {
        return Ok(None);
    }
    choose(memory_bytes()?).map(str::to_string).map(Some)
}

fn choose(bytes: u64) -> Result<&'static str, String> {
    match bytes / (1024 * 1024 * 1024) {
        128.. => Ok(LARGE),
        65.. => Ok(MEDIUM),
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

#[cfg(target_os = "windows")]
fn memory_bytes() -> Result<u64, String> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: MEMORYSTATUSEX is a plain-old-data struct with no invalid bit
    // patterns, so a zeroed value is valid; dwLength must be the struct size
    // before the call, which the next line sets.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: the pointer refers to a live, correctly sized, writable buffer
    // with dwLength initialised as the API requires.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return Err("Cannot read local memory for private model selection".into());
    }
    Ok(status.ullTotalPhys)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn memory_bytes() -> Result<u64, String> {
    Err("Private model selection is not supported on this platform yet".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendations_follow_total_memory_rungs() {
        let gib = 1024 * 1024 * 1024;
        for size in [0, 4, 7] {
            assert!(choose(size * gib).is_err());
        }
        for size in [8, 16, 24, 32, 48, 64] {
            assert_eq!(choose(size * gib).unwrap(), SMALL);
        }
        for size in [65, 80, 96, 127] {
            assert_eq!(choose(size * gib).unwrap(), MEDIUM);
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

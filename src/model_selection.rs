//! Conservative launcher policy, not Mesh discovery or an inference-engine policy.
//! Joining preserves local serving participation.
use crate::settings::Connection;

// Pinned OpenClaw recipes: d08c80a126097113d1b412d67aaa173aa889b4b8,
// extensions/llama-cpp/src/model-catalog.ts. Budgets include 64K context/runtime.
const GIB: u64 = 1024 * 1024 * 1024;
const SMALL: &str = "unsloth/Qwen3.5-4B-GGUF@e87f176479d0855a907a41277aca2f8ee7a09523:Q4_K_M";
const MEDIUM: &str = "unsloth/Qwen3.5-9B-GGUF@3885219b6810b007914f3a7950a8d1b469d598a5:Q4_K_M";
const GEMMA: &str = "unsloth/gemma-4-12b-it-GGUF@fc034cfff751157913579611efad8462ac1be606:Q4_K_M";
const LARGE: &str = "unsloth/Qwen3.8-27B-GGUF@4ca720788d1e01f1bff70c033e0d0028fd02e502:UD-Q4_K_M";

pub fn local_model(connection: &Connection) -> Result<Option<String>, String> {
    if !matches!(connection, Connection::Private { .. }) {
        return Ok(None);
    }
    let total = memory_bytes()?;
    let available = crate::model_hardware::available_memory()?;
    // Only Apple Silicon is positively identified here as unified GPU memory.
    // Other platforms stay on CPU recipes until a usable GPU budget is probed.
    let accelerated = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    choose(total, available, accelerated)
        .map(str::to_string)
        .map(Some)
}

fn choose(total: u64, available: u64, accelerated: bool) -> Result<&'static str, String> {
    let budget = available.min(total.saturating_sub((2 * GIB).max(total / 4)));
    for (model, floor, required, gpu) in [
        (LARGE, 32, 22, true),
        (GEMMA, 24, 12, true),
        (MEDIUM, 16, 10, false),
        (SMALL, 8, 6, false),
    ] {
        if total >= floor * GIB && budget >= required * GIB && (!gpu || accelerated) {
            return Ok(model);
        }
    }
    Err("Not enough available memory for an automatic model with 64K context. Close other applications or configure a model explicitly.".into())
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
    fn recipes_reserve_context_and_host_headroom() {
        for (size, expected) in [
            (8, SMALL),
            (16, MEDIUM),
            (24, GEMMA),
            (32, LARGE),
            (128, LARGE),
        ] {
            assert_eq!(choose(size * GIB, size * GIB, true).unwrap(), expected);
        }
        assert_eq!(choose(128 * GIB, 128 * GIB, false).unwrap(), MEDIUM);
        assert_eq!(choose(32 * GIB, 12 * GIB, true).unwrap(), GEMMA);
        assert_eq!(choose(32 * GIB, 10 * GIB, true).unwrap(), MEDIUM);
        assert_eq!(choose(32 * GIB, 6 * GIB, true).unwrap(), SMALL);
        assert!(choose(128 * GIB, 5 * GIB, true).is_err());
        assert!(choose(7 * GIB, 7 * GIB, true).is_err());
        assert_eq!(local_model(&Connection::Automatic).unwrap(), None);
    }
}

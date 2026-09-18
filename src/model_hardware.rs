//! Available host memory for conservative automatic selection. No credential IO.

#[cfg(target_os = "macos")]
pub fn available_memory() -> Result<u64, String> {
    let output = std::process::Command::new("/usr/bin/vm_stat")
        .output()
        .map_err(|e| format!("Cannot inspect available memory: {e}"))?;
    if !output.status.success() {
        return Err("Cannot inspect available memory".into());
    }
    parse_vm_stat(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "Cannot parse available memory".into())
}

#[cfg(any(target_os = "macos", test))]
fn parse_vm_stat(text: &str) -> Option<u64> {
    let page_size = text
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    let pages = |name: &str| -> Option<u64> {
        text.lines()
            .find_map(|line| line.strip_prefix(name))?
            .trim()
            .trim_end_matches('.')
            .parse()
            .ok()
    };
    // Inactive pages are reclaimable; purgeable pages overlap and are not added.
    pages("Pages free:")?
        .checked_add(pages("Pages inactive:")?)?
        .checked_mul(page_size)
}

#[cfg(target_os = "linux")]
pub fn available_memory() -> Result<u64, String> {
    let text = std::fs::read_to_string("/proc/meminfo").map_err(|e| e.to_string())?;
    let available = text
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse::<u64>().ok())
        .and_then(|v| v.checked_mul(1024))
        .ok_or("Cannot inspect available memory")?;
    // Conservative cgroup-v2 limit when running in the usual container mount.
    let limit = std::fs::read_to_string("/sys/fs/cgroup/memory.max")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok());
    if let Some(limit) = limit {
        let used = std::fs::read_to_string("/sys/fs/cgroup/memory.current")
            .map_err(|e| e.to_string())?
            .trim()
            .parse::<u64>()
            .map_err(|e| e.to_string())?;
        return Ok(available.min(limit.saturating_sub(used)));
    }
    Ok(available)
}

#[cfg(target_os = "windows")]
pub fn available_memory() -> Result<u64, String> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: zero is valid for the POD structure; length and pointer are valid.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err("Cannot inspect available memory".into());
    }
    Ok(status.ullAvailPhys)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn available_memory() -> Result<u64, String> {
    Err("Automatic memory sizing is unavailable on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vm_stat_counts_reclaimable_pages_once() {
        assert_eq!(parse_vm_stat("Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 10.\nPages inactive: 20.\nPages purgeable: 15."), Some(30 * 16384));
        assert_eq!(parse_vm_stat("bad"), None);
        assert_eq!(
            parse_vm_stat("page size of 4096 bytes\nPages free: 1."),
            None
        );
    }
}

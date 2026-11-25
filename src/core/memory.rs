//! How much memory the caches may use, from the RAM installed: a 4 GB
//! laptop should not be asked for 2 GB, and a 32 GB desktop can keep more
//! pictures ready for going back.

/// Memory for the three caches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    /// Screen-size pictures ([`super::library::Library`]): the one that
    /// decides how far back is still instant.
    pub screen: usize,
    /// Full-resolution pixels of the current picture and the next few.
    pub full: usize,
    /// Files read ahead into memory, not yet decoded.
    pub read: usize,
}

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

/// A quarter of the RAM, between 1 and 6 GB; unknown RAM counts as 8 GB.
/// Full pixels and read-ahead only need room for what they hold (the
/// current picture and three more; ten files), so they stop at 1 GB and the
/// rest goes to screen-size pictures.
pub fn budgets_for(ram: Option<u64>) -> Budgets {
    let pool = (ram.unwrap_or(8 * GB) / 4).clamp(GB, 6 * GB);
    let full = (pool * 30 / 100).min(GB);
    let read = (pool / 4).min(GB);
    Budgets {
        screen: (pool - full - read) as usize,
        full: full as usize,
        read: read as usize,
    }
}

pub fn budgets() -> Budgets {
    budgets_for(installed_ram())
}

/// Physical memory installed, in bytes.
#[cfg(windows)]
pub fn installed_ram() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut s: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    s.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `s` is a properly sized, writable MEMORYSTATUSEX.
    let ok = unsafe { GlobalMemoryStatusEx(&mut s) };
    (ok != 0 && s.ullTotalPhys > 0).then_some(s.ullTotalPhys)
}

#[cfg(not(windows))]
pub fn installed_ram() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = info
        .lines()
        .find(|l| l.starts_with("MemTotal:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// "1.2 GB" / "640 MB".
pub fn human(bytes: usize) -> String {
    let b = bytes as u64;
    if b >= GB {
        format!("{:.1} GB", b as f64 / GB as f64)
    } else {
        format!("{} MB", b / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(b: Budgets) -> u64 {
        (b.screen + b.full + b.read) as u64
    }

    #[test]
    fn budgets_follow_the_installed_ram() {
        // 8 GB: about what the fixed sizes were (1 GB + 640 MB + 512 MB).
        let b = budgets_for(Some(8 * GB));
        assert_eq!(total(b), 2 * GB);
        assert_eq!(b.read as u64, 512 * MB);
        assert!(
            b.full as u64 >= 600 * MB && b.screen as u64 >= 900 * MB,
            "{b:?}"
        );
        // Unknown RAM: the same.
        assert_eq!(budgets_for(None), b);
        // 4 GB: half as much, never below 1 GB in all.
        assert_eq!(total(budgets_for(Some(4 * GB))), GB);
        assert_eq!(total(budgets_for(Some(2 * GB))), GB);
        // 32 GB and up: 6 GB, the extra going to screen-size pictures.
        let big = budgets_for(Some(64 * GB));
        assert_eq!(total(big), 6 * GB);
        assert_eq!((big.full as u64, big.read as u64), (GB, GB));
        assert_eq!(big.screen as u64, 4 * GB);
    }

    #[test]
    fn this_machine_reports_its_ram() {
        let ram = installed_ram().expect("RAM known");
        assert!(ram > 256 * MB, "{ram}");
        assert_eq!(human(640 * MB as usize), "640 MB");
        assert_eq!(human(3 * GB as usize / 2), "1.5 GB");
    }
}

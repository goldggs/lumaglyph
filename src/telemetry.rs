/// Lightweight, zero-dependency cross-platform Resident Set Size (RSS) reader
/// for Linux, macOS, and Windows so users can verify LumaGlyph's low memory footprint live.
pub fn current_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        parse_linux_statm(&statm)
    }

    #[cfg(target_os = "macos")]
    {
        #[repr(C)]
        struct MachTaskBasicInfo {
            virtual_size: u64,
            resident_size: u64,
            resident_size_max: u64,
            user_time: [u32; 2],
            system_time: [u32; 2],
            policy: i32,
            suspend_count: i32,
        }
        extern "C" {
            fn mach_task_self() -> u32;
            fn task_info(
                target_task: u32,
                flavor: u32,
                task_info_out: *mut MachTaskBasicInfo,
                task_info_outCnt: *mut u32,
            ) -> i32;
        }
        const MACH_TASK_BASIC_INFO: u32 = 20;
        let mut info = std::mem::MaybeUninit::<MachTaskBasicInfo>::zeroed();
        let mut count =
            (std::mem::size_of::<MachTaskBasicInfo>() / std::mem::size_of::<u32>()) as u32;
        // SAFETY: Querying our own Mach task handle with properly sized output struct.
        let kr = unsafe {
            task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                info.as_mut_ptr(),
                &mut count,
            )
        };
        if kr == 0 {
            // SAFETY: task_info returned KERN_SUCCESS, so it filled the struct.
            let info = unsafe { info.assume_init() };
            Some(info.resident_size)
        } else {
            None
        }
    }

    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct ProcessMemoryCounters {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> *mut core::ffi::c_void;
            fn K32GetProcessMemoryInfo(
                process: *mut core::ffi::c_void,
                ppsmem_counters: *mut ProcessMemoryCounters,
                cb: u32,
            ) -> i32;
        }
        let mut pmc = std::mem::MaybeUninit::<ProcessMemoryCounters>::zeroed();
        let cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
        // SAFETY: Standard Win32 K32GetProcessMemoryInfo call on GetCurrentProcess pseudo-handle.
        let ok = unsafe {
            (*pmc.as_mut_ptr()).cb = cb;
            K32GetProcessMemoryInfo(GetCurrentProcess(), pmc.as_mut_ptr(), cb)
        };
        if ok != 0 {
            // SAFETY: K32GetProcessMemoryInfo returned non-zero, so it filled the struct.
            let pmc = unsafe { pmc.assume_init() };
            Some(pmc.working_set_size as u64)
        } else {
            None
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

pub fn format_rss_mb() -> String {
    match current_rss_bytes() {
        Some(bytes) => format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)),
        None => "N/A".to_string(),
    }
}

/// Parse `/proc/self/statm` into private resident bytes.
///
/// Field 1 is total pages, field 2 is resident pages, field 3 is shared pages.
/// Shared pages are subtracted because they are memory-mapped libraries (GTK, GL,
/// the C runtime) that every app on the system shares; counting them would make
/// LumaGlyph look far heavier than it is and would not match what Windows Task
/// Manager or macOS Activity Monitor report.
///
/// Kept separate from the `/proc` read so the arithmetic can be tested on any OS.
pub fn parse_linux_statm(statm: &str) -> Option<u64> {
    const PAGE_SIZE: u64 = 4096;
    let mut cols = statm.split_whitespace();
    let _total_pages: u64 = cols.next()?.parse().ok()?;
    let rss_pages: u64 = cols.next()?.parse().ok()?;
    let shared_pages: u64 = cols.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some(rss_pages.saturating_sub(shared_pages) * PAGE_SIZE)
}

#[cfg(test)]
mod telemetry_tests {
    use super::*;

    #[test]
    fn statm_parses_a_realistic_line() {
        // total, resident, shared, then text/lib/data/dirty pages.
        let line = "20000 5000 1000 400 0 900 0\n";
        let bytes = parse_linux_statm(line).unwrap();
        assert_eq!(bytes, (5000 - 1000) * 4096);
    }

    #[test]
    fn statm_subtracts_shared_library_pages() {
        let with_shared = parse_linux_statm("1000 1000 600 0 0 0 0").unwrap();
        let without_shared = parse_linux_statm("1000 1000 0 0 0 0 0").unwrap();
        assert!(with_shared < without_shared);
        assert_eq!(without_shared - with_shared, 600 * 4096);
    }

    #[test]
    fn statm_never_underflows_when_shared_exceeds_resident() {
        // A kernel that reports odd numbers must not wrap into an enormous value.
        let bytes = parse_linux_statm("500 100 900 0 0 0 0").unwrap();
        assert_eq!(bytes, 0);
    }

    #[test]
    fn statm_tolerates_two_field_lines() {
        // The third field is optional in the parser.
        assert_eq!(parse_linux_statm("500 200"), Some(200 * 4096));
    }

    #[test]
    fn statm_rejects_garbage() {
        for junk in ["", "   ", "abc", "1 two 3", "-5 10 2"] {
            assert_eq!(parse_linux_statm(junk), None, "accepted {junk:?}");
        }
    }

    #[test]
    fn rss_reading_is_available_on_supported_platforms() {
        // Linux, macOS and Windows all have a real implementation.
        let value = current_rss_bytes();
        assert!(value.is_some(), "no RSS reading on this platform");
        let bytes = value.unwrap();
        assert!(bytes > 0, "RSS reported as zero bytes");
        // The test process alone is at least a few hundred kilobytes.
        assert!(bytes > 256 * 1024, "RSS implausibly small: {bytes}");
    }

    #[test]
    fn formatted_report_is_stable_and_readable() {
        let text = format_rss_mb();
        assert!(
            text.ends_with(" MB") || text == "N/A",
            "unexpected format: {text}"
        );
        if text != "N/A" {
            let number: f64 = text.trim_end_matches(" MB").parse().expect("not a number");
            assert!(
                number > 0.0 && number < 100_000.0,
                "implausible report: {text}"
            );
        }
    }

    #[test]
    fn repeated_reads_do_not_drift_wildly() {
        let first = current_rss_bytes().unwrap();
        let second = current_rss_bytes().unwrap();
        // Memory can grow, but it cannot halve between two calls.
        assert!(second * 2 > first, "RSS collapsed from {first} to {second}");
    }
}

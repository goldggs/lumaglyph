/// Lightweight, zero-dependency cross-platform Resident Set Size (RSS) reader
/// for Linux, macOS, and Windows so users can verify LumaGlyph's low memory footprint live.
pub fn current_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let mut cols = statm.split_whitespace();
        let _total_pages: u64 = cols.next()?.parse().ok()?;
        let rss_pages: u64 = cols.next()?.parse().ok()?;
        let shared_pages: u64 = cols.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        // Report private resident memory (`RssAnon` = resident - shared OS `.so` files),
        // matching Windows Private Working Set & macOS Activity Monitor "Memory".
        Some(rss_pages.saturating_sub(shared_pages) * 4096)
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

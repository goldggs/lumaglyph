#![cfg_attr(all(target_os = "windows", not(test)), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod win_console {
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
    const STD_ERROR_HANDLE: u32 = (-12i32) as u32;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(dwProcessId: u32) -> i32;
        fn GetStdHandle(nStdHandle: u32) -> isize;
    }

    pub fn has_valid_stderr() -> bool {
        // SAFETY: GetStdHandle is always safe to call; it returns a pseudo-handle
        // or INVALID_HANDLE_VALUE and never transfers ownership.
        let h = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        h != 0 && h != INVALID_HANDLE_VALUE
    }

    /// Attaches to an existing parent shell (`cmd.exe` / PowerShell / Windows Terminal) ONLY for
    /// headless CLI subcommands when standard handles are not already redirected to pipes.
    /// Never calls `AllocConsole`, so launching `lumaglyph.exe` as a GUI app never opens a terminal.
    pub fn attach_parent_if_needed() {
        // SAFETY: AttachConsole only links this process to an already-existing
        // parent console. It allocates no memory and takes no pointers, and a
        // failure is ignored, so there is nothing to invalidate.
        unsafe {
            let out_h = GetStdHandle(STD_OUTPUT_HANDLE);
            let err_h = GetStdHandle(STD_ERROR_HANDLE);
            let out_valid = out_h != 0 && out_h != INVALID_HANDLE_VALUE;
            let err_valid = err_h != 0 && err_h != INVALID_HANDLE_VALUE;
            if !out_valid && !err_valid {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }
}

fn main() {
    let _ = lumaglyph::updater::cleanup_stale_old_executables();

    #[cfg(target_os = "windows")]
    let cli_mode = {
        let raw_args: Vec<String> = std::env::args().collect();
        let attach = lumaglyph::cli::should_attach_parent_console(&raw_args);
        if attach {
            win_console::attach_parent_if_needed();
        }
        attach
    };

    if let Err(err) = lumaglyph::cli::run_entry() {
        #[cfg(target_os = "windows")]
        if !cli_mode && !win_console::has_valid_stderr() {
            let _ = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("LumaGlyph")
                .set_description(&err)
                .show();
            std::process::exit(1);
        }

        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

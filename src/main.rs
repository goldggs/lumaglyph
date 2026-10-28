// Hide console window on Windows ONLY when launched without CLI arguments?
// Note: Keeping standard subsystem lets both `lumaglyph` GUI and `lumaglyph image/video/bench` CLI
// print clean help/progress on Windows, macOS, and Linux.

fn main() {
    if let Err(err) = lumaglyph::cli::run_entry() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

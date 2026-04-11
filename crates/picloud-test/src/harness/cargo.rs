/// Find the cargo binary.
///
/// Checks PATH first (via `std::process::Command` which resolves PATH),
/// then falls back to `$HOME/.cargo/bin/cargo` for environments where
/// cargo is installed via rustup but the shell profile hasn't been sourced.
pub fn cargo_bin() -> std::path::PathBuf {
    // Quick check: can we find cargo on PATH?
    if let Ok(output) = std::process::Command::new("cargo")
        .arg("--version")
        .output()
    {
        if output.status.success() {
            return std::path::PathBuf::from("cargo");
        }
    }

    // Fallback: try ~/.cargo/bin/cargo (rustup default location)
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let fallback = std::path::PathBuf::from(format!("{home}/.cargo/bin/cargo"));
    if fallback.exists() {
        return fallback;
    }

    // Last resort: return "cargo" and let the caller handle the error
    std::path::PathBuf::from("cargo")
}

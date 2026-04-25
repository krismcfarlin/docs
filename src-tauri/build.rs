fn main() {
    // Load OAuth credentials from gitignored secrets file.
    // Fall back to empty strings — user must supply via Settings if not present.
    if let Ok(contents) = std::fs::read_to_string(".oauth_secrets") {
        for line in contents.lines() {
            if let Some((key, val)) = line.split_once('=') {
                println!("cargo:rustc-env={}={}", key.trim(), val.trim());
            }
        }
    }
    tauri_build::build()
}

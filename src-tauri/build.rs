use std::path::Path;

/// Read KEY=VALUE lines from a .env file (comments and blank lines skipped, quotes stripped).
fn read_env(path: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            Some((k.trim().trim_start_matches("export ").to_string(), v.trim().trim_matches(|c| c == '"' || c == '\'').to_string()))
        })
        .collect()
}

fn main() {
    // Google sign-in details come from ../.env (or the build's environment) and are compiled in,
    // so users never have to type them. See .env.example.
    let env_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".env");
    println!("cargo:rerun-if-changed={}", env_file.display());
    println!("cargo:rerun-if-env-changed=GOOGLE_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=GOOGLE_CLIENT_SECRET");
    let file = read_env(&env_file);
    for key in ["GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET"] {
        let value = std::env::var(key).ok().filter(|v| !v.is_empty()).or_else(|| file.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())).unwrap_or_default();
        println!("cargo:rustc-env=JARVIS_{key}={value}");
    }
    tauri_build::build()
}

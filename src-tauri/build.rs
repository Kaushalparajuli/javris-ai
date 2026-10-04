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

/// Compile the native audio helper (helpers/jarvis-audio.swift) into binaries/, where Tauri picks it
/// up as a sidecar: copied next to the executable in development, and into Jarvis.app/Contents/MacOS
/// in the bundle. Only rebuilt when the Swift source is newer than the binary.
fn build_audio_helper() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = manifest.join("helpers").join("jarvis-audio.swift");
    println!("cargo:rerun-if-changed={}", source.display());
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=JARVIS_TARGET={target}");
    if !target.contains("apple-darwin") {
        return;
    }
    let out = manifest.join("binaries").join(format!("jarvis-audio-{target}"));
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if let (Some(built), Some(src)) = (modified(&out), modified(&source)) {
        if built >= src {
            return;
        }
    }
    let _ = std::fs::create_dir_all(out.parent().unwrap());
    let arch = if target.starts_with("x86_64") { "x86_64" } else { "arm64" };
    // Build to a temporary name and move it into place, so a failed build never leaves half a binary.
    let tmp = out.with_extension("building");
    let status = std::process::Command::new("xcrun")
        .args(["swiftc", "-O", "-target", &format!("{arch}-apple-macos13.0")])
        .arg(&source)
        .arg("-o")
        .arg(&tmp)
        .status();
    match status {
        Ok(s) if s.success() => std::fs::rename(&tmp, &out).expect("couldn't move the audio helper into place"),
        Ok(_) => panic!("The audio helper (helpers/jarvis-audio.swift) didn't compile; see the errors above."),
        Err(e) => panic!("Couldn't run swiftc to build the audio helper ({e}). Install Xcode or the command line tools: xcode-select --install"),
    }
}

fn main() {
    build_audio_helper();
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

/// Build script that injects the git hash at compile time.
///
/// Priority order:
/// 1. `GIT_HASH` env var (set by Docker build-arg or CI)
/// 2. `git rev-parse --short HEAD` (auto-detected from the repo)
/// 3. `"unknown"` fallback (not in a git repo or git not available)
fn main() {
    println!("cargo:rerun-if-env-changed=GIT_HASH");
    println!("cargo:rerun-if-changed=../../.git/HEAD");

    // If GIT_HASH is already set (for example via Docker build-arg), honor it.
    if let Ok(hash) = std::env::var("GIT_HASH") {
        println!("cargo:rustc-env=GIT_HASH={hash}");
        return;
    }

    // Try to detect from git for local development builds.
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output();

    let hash = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::from("unknown"),
    };

    println!("cargo:rustc-env=GIT_HASH={hash}");
}

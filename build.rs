use std::process::Command;

fn main() {
    // Embed the short git commit hash at compile time.
    // Falls back to "unknown" if git is not available or not in a repo.
    let hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=PSMUX_GIT_HASH={}", hash);

    // Only re-run when the git HEAD changes (new commit, checkout, etc.)
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");
}

//! On Windows this script embeds a Win32 resource block into the executable:
//! the application icon plus a VS_VERSIONINFO record. Without it Windows has
//! no name, description or version to show for the process, so Task Manager,
//! the taskbar, alt-tab and consent dialogs such as the one 1Password raises
//! all display a nameless generic entry (issue #620).

use std::process::Command;

fn main() {
    embed_windows_resources();

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

/// Embed the icon and the VS_VERSIONINFO block (Windows hosts only).
///
/// The `#[cfg(windows)]` here is the *host* platform, which is what gates the
/// `winresource` build-dependency in `Cargo.toml`. The target is checked
/// separately below so a Windows host cross-compiling to a non-Windows target
/// still produces a clean build.
#[cfg(windows)]
fn embed_windows_resources() {
    use std::path::Path;
    use winresource::{VersionInfo, WindowsResource};

    // Only PE targets carry Win32 resources.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let icon = Path::new("assets").join("psmux.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    // A build that quietly dropped the icon would reintroduce #620, so treat a
    // missing asset as a build failure rather than carrying on without it.
    assert!(
        icon.is_file(),
        "{} is missing; it is required to embed the application icon",
        icon.display()
    );

    let version = env!("CARGO_PKG_VERSION");
    let mut res = WindowsResource::new();
    res.set_icon(icon.to_str().expect("assets/psmux.ico path is not UTF-8"));
    res.set("ProductName", "psmux")
        .set("FileDescription", "psmux terminal multiplexer")
        .set("CompanyName", "psmux")
        .set("InternalName", "psmux")
        // psmux, pmux and tmux are the same image; psmux.exe is canonical.
        .set("OriginalFilename", "psmux.exe")
        .set("LegalCopyright", "Copyright (c) psmux contributors. MIT licensed.")
        .set("FileVersion", version)
        .set("ProductVersion", version);

    // VS_FIXEDFILEINFO wants a packed u64: major.minor.patch.build. The
    // pre-release suffix (3.4.0-ohboy) stays in the string fields only.
    let packed = packed_version(version);
    res.set_version_info(VersionInfo::FILEVERSION, packed);
    res.set_version_info(VersionInfo::PRODUCTVERSION, packed);

    if let Err(e) = res.compile() {
        panic!("failed to embed the Windows resource block (icon / version info): {e}");
    }
}

#[cfg(not(windows))]
fn embed_windows_resources() {}

/// Pack `major.minor.patch` into the u64 layout VS_FIXEDFILEINFO expects.
/// Non-numeric or missing components fall back to 0.
#[cfg(windows)]
fn packed_version(version: &str) -> u64 {
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u64>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    (major << 48) | (minor << 32) | (patch << 16)
}

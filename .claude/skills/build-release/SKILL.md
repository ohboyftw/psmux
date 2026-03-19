---
name: build-release
description: Build psmux release binaries, create GitHub releases, and package for distribution (Chocolatey, crates.io). Use when the user mentions "release", "build release", "publish", "deploy", "bump version", "ship it", "create release", "package for chocolatey", or "cargo publish". Also trigger for version bumping or changelog generation.
---

# Build & Release Workflow for psmux

## Pre-release Checklist

Before building a release, verify all of these pass:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## Version Bump

1. Update version in `Cargo.toml`
2. Update version in `packages/chocolatey/psmux.nuspec` (if it exists)
3. Update `CHANGELOG.md` (create one if missing — use Keep a Changelog format)
4. Commit with message: `chore: bump version to vX.Y.Z`

## Build Release Binary

```bash
cargo build --release
```

The binary will be at `target/release/psmux.exe`.

## Packaging

### GitHub Release
1. Create a git tag: `git tag vX.Y.Z`
2. Push tag: `git push origin vX.Y.Z`
3. The `.github/workflows/` CI should handle the rest
4. If manual: create a `.zip` containing `psmux.exe`, `pmux.exe`, `tmux.exe` (copies of the same binary)

### Chocolatey
1. Navigate to `packages/chocolatey/`
2. Update the nuspec version and release notes
3. Verify the `chocolateyInstall.ps1` script points to the correct download URL
4. Run `choco pack` to create the `.nupkg`
5. Test locally: `choco install psmux --source .`
6. Push: `choco push psmux.X.Y.Z.nupkg --source https://push.chocolatey.org/`

### crates.io
1. Ensure `Cargo.toml` metadata is complete (description, license, repository)
2. Dry run: `cargo publish --dry-run`
3. Publish: `cargo publish`

## Post-release
- Verify the install script works: `irm https://raw.githubusercontent.com/marlocarlo/psmux/master/scripts/install.ps1 | iex`
- Update README if any install instructions changed
- Announce the release with a summary of changes

use super::*;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn load_err(dir: &Path) -> String {
    match load_conpty_from_dir(dir, ConPtySource::Directory) {
        Ok(_) => panic!("{} loaded", dir.display()),
        Err(reason) => reason,
    }
}

#[test]
fn no_directory_named_or_bundled_uses_kernel32() {
    assert_eq!(resolve_conpty(None, None).source, ConPtySource::Kernel32);
}

#[test]
fn missing_bundled_directory_uses_kernel32() {
    let missing = std::env::temp_dir().join("psmux-no-such-conpty-dir-3f9a1c");
    assert!(!missing.exists());
    assert_eq!(
        resolve_conpty(None, Some(missing)).source,
        ConPtySource::Kernel32
    );
}

#[test]
fn empty_env_value_means_not_named() {
    assert_eq!(conpty_dir_from_env(Some(OsString::new())), None);
    assert_eq!(conpty_dir_from_env(None), None);
    assert_eq!(
        conpty_dir_from_env(Some(OsString::from("C:\\x"))),
        Some(PathBuf::from("C:\\x"))
    );
}

#[test]
fn named_directory_that_does_not_exist_falls_back_to_kernel32() {
    let missing = std::env::temp_dir().join("psmux-no-such-conpty-dir-3f9a1c");
    assert!(!missing.exists());
    let err = load_err(&missing);
    assert!(err.contains("is not a directory"), "{}", err);
    assert_eq!(
        resolve_conpty(Some(missing), None).source,
        ConPtySource::Kernel32
    );
}

#[test]
fn named_directory_without_dll_falls_back_to_kernel32() {
    let dir = scratch_dir("psmux-conpty-empty-3f9a1c");
    let err = load_err(&dir);
    assert!(err.contains("conpty.dll does not exist"), "{}", err);
    assert_eq!(
        resolve_conpty(Some(dir), None).source,
        ConPtySource::Kernel32
    );
}

#[test]
fn named_directory_without_openconsole_falls_back_to_kernel32() {
    let dir = scratch_dir("psmux-conpty-no-host-3f9a1c");
    std::fs::write(dir.join("conpty.dll"), b"x").unwrap();
    let err = load_err(&dir);
    assert!(err.contains("OpenConsole.exe does not exist"), "{}", err);
}

#[test]
fn named_directory_with_bad_dll_falls_back_to_kernel32() {
    let dir = scratch_dir("psmux-conpty-bad-3f9a1c");
    std::fs::write(dir.join("conpty.dll"), b"not a PE image at all").unwrap();
    std::fs::write(dir.join("OpenConsole.exe"), b"x").unwrap();
    let err = load_err(&dir);
    assert!(err.contains("LoadLibraryExW"), "{}", err);
    assert_eq!(
        resolve_conpty(Some(dir), None).source,
        ConPtySource::Kernel32
    );
}

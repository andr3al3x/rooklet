use super::*;
use std::os::unix::fs::symlink;

#[test]
fn missing_state_destination_is_validated_without_creation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing-state");
    validate_existing_destination(&path, true).unwrap();
    assert!(!path.exists());
}

#[test]
fn state_directory_symlink_is_rejected_even_without_a_state_file() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target");
    fs::create_dir(&target).unwrap();
    let link = directory.path().join("state");
    symlink(&target, &link).unwrap();
    let error = validate_existing_destination(&link, true).unwrap_err();
    assert!(error.to_string().contains("symlink"));
    assert!(!target.join("network.json").exists());
}

#[test]
fn state_lock_symlink_or_nonfile_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("unrelated");
    fs::write(&target, b"preserve").unwrap();
    let link = directory.path().join(".network.lock");
    symlink(&target, &link).unwrap();
    assert!(validate_existing_destination(&link, false).is_err());
    assert_eq!(fs::read(target).unwrap(), b"preserve");
    assert!(validate_existing_destination(directory.path(), false).is_err());
}

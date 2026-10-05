#![cfg(target_os = "macos")]

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::{TempDir, tempdir};

struct Package {
    root: TempDir,
    archive: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
}

impl Package {
    fn new() -> Self {
        let root = tempdir().unwrap();
        let archive = root.path().join("extracted archive");
        let home = root.path().join("user home");
        let cwd = root.path().join("unrelated working directory");
        for path in [&archive, &home, &cwd] {
            fs::create_dir(path).unwrap();
        }
        for name in ["install.sh", "uninstall.sh"] {
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("scripts")
                    .join(name),
                archive.join(name),
            )
            .unwrap();
        }
        fs::copy(env!("CARGO_BIN_EXE_xield"), archive.join("xield")).unwrap();
        Self {
            root,
            archive,
            home,
            cwd,
        }
    }

    fn run(&self, script: &str, args: &[&str]) -> Output {
        Command::new("/bin/sh")
            .arg(self.archive.join(script))
            .args(args)
            .env("HOME", &self.home)
            // A binary installation must not invoke Xcode developer-tool shims.
            .env(
                "DEVELOPER_DIR",
                self.root.path().join("absent developer tools"),
            )
            .current_dir(&self.cwd)
            .output()
            .unwrap()
    }

    fn bin_dir(&self) -> PathBuf {
        self.home.join(".local/bin")
    }

    fn assert_no_staging_files(&self, directory: &Path) {
        if !directory.exists() {
            return;
        }
        assert!(fs::read_dir(directory).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".xield-install.")
        }));
    }
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn archive_installs_from_an_unrelated_directory_to_the_user_bin() {
    let package = Package::new();
    assert_success(package.run("install.sh", &[]));
    let installed = package.bin_dir().join("xield");
    assert_eq!(
        fs::read(&installed).unwrap(),
        fs::read(package.archive.join("xield")).unwrap()
    );
    assert_eq!(
        fs::metadata(&installed).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_success(Command::new(installed).arg("--version").output().unwrap());
    package.assert_no_staging_files(&package.bin_dir());
}

#[test]
fn explicit_source_and_destination_support_spaces_and_upgrades() {
    let package = Package::new();
    let bin_dir = package.root.path().join("custom bin directory");
    fs::create_dir(&bin_dir).unwrap();
    let installed = bin_dir.join("xield");
    fs::write(&installed, "old executable").unwrap();
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o600)).unwrap();
    let source = package.cwd.join("source executable");
    fs::copy(env!("CARGO_BIN_EXE_xield"), &source).unwrap();
    // A relative override is resolved from the caller's working directory.
    let args = [
        "--binary",
        "source executable",
        "--bin-dir",
        bin_dir.to_str().unwrap(),
    ];
    assert_success(package.run("install.sh", &args));
    assert_success(package.run("install.sh", &args));
    assert_eq!(fs::read(&installed).unwrap(), fs::read(source).unwrap());
    assert_eq!(
        fs::metadata(&installed).unwrap().permissions().mode() & 0o777,
        0o755
    );
    package.assert_no_staging_files(&bin_dir);
}

#[test]
fn install_and_uninstall_refuse_symlinks_and_directories() {
    let package = Package::new();
    fs::create_dir_all(package.bin_dir()).unwrap();
    let destination = package.bin_dir().join("xield");
    let unrelated = package.root.path().join("unrelated executable");
    fs::write(&unrelated, "keep me").unwrap();
    symlink(&unrelated, &destination).unwrap();
    for script in ["install.sh", "uninstall.sh"] {
        assert!(!package.run(script, &[]).status.success());
        assert!(
            fs::symlink_metadata(&destination)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep me");
    }
    fs::remove_file(&destination).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep"), "directory contents").unwrap();
    for script in ["install.sh", "uninstall.sh"] {
        assert!(!package.run(script, &[]).status.success());
        assert_eq!(
            fs::read_to_string(destination.join("keep")).unwrap(),
            "directory contents"
        );
    }
    package.assert_no_staging_files(&package.bin_dir());
}

#[test]
fn invalid_sources_leave_the_previous_installation_intact() {
    let package = Package::new();
    fs::create_dir_all(package.bin_dir()).unwrap();
    let installed = package.bin_dir().join("xield");
    fs::write(&installed, "previous version").unwrap();
    let source = package.archive.join("xield");
    for mode in [0o755, 0o644] {
        fs::write(&source, "not a Mach-O executable").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(mode)).unwrap();
        assert!(!package.run("install.sh", &[]).status.success());
        assert_eq!(fs::read_to_string(&installed).unwrap(), "previous version");
        package.assert_no_staging_files(&package.bin_dir());
    }
    fs::remove_file(source).unwrap();
    assert!(!package.run("install.sh", &[]).status.success());
    assert_eq!(fs::read_to_string(installed).unwrap(), "previous version");
}

#[test]
fn mach_o_nonexecutables_and_other_architectures_preserve_the_previous_installation() {
    let package = Package::new();
    fs::create_dir_all(package.bin_dir()).unwrap();
    let installed = package.bin_dir().join("xield");
    fs::write(&installed, "previous version").unwrap();
    let source = package.archive.join("xield");
    let executable = fs::read(&source).unwrap();
    // Both supported macOS targets use a little-endian 64-bit Mach-O header.
    assert_eq!(&executable[..4], &0xfeed_facfu32.to_le_bytes());
    assert_eq!(&executable[12..16], &2u32.to_le_bytes()); // MH_EXECUTE
    for file_type in [1u32, 6u32] {
        // Preserve a real executable's structure, changing only MH_OBJECT/MH_DYLIB.
        // These fixtures exercise header validation and are never executed.
        let mut fixture = executable.clone();
        fixture[12..16].copy_from_slice(&file_type.to_le_bytes());
        fs::write(&source, fixture).unwrap();
        let output = package.run("install.sh", &[]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("executable"));
        assert_eq!(fs::read_to_string(&installed).unwrap(), "previous version");
        package.assert_no_staging_files(&package.bin_dir());
    }

    let original_cpu = u32::from_le_bytes(executable[4..8].try_into().unwrap());
    let (other_cpu, other_subtype, other_arch) = match original_cpu {
        0x0100_000c => (0x0100_0007u32, 3u32, "x86_64"),
        0x0100_0007 => (0x0100_000cu32, 0u32, "arm64"),
        unexpected => panic!("unexpected Mach-O CPU type: {unexpected:#x}"),
    };
    let mut fixture = executable;
    fixture[4..8].copy_from_slice(&other_cpu.to_le_bytes());
    fixture[8..12].copy_from_slice(&other_subtype.to_le_bytes());
    fs::write(&source, fixture).unwrap();
    let architecture = Command::new("/usr/bin/lipo")
        .arg("-archs")
        .arg(&source)
        .output()
        .unwrap();
    assert!(architecture.status.success());
    assert_eq!(
        String::from_utf8(architecture.stdout).unwrap().trim(),
        other_arch
    );
    let output = package.run("install.sh", &[]);
    let translated = Command::new("/usr/sbin/sysctl")
        .args(["-n", "sysctl.proc_translated"])
        .output()
        .unwrap();
    if translated.status.success() && translated.stdout.trim_ascii() == b"1" {
        // A Rosetta terminal accepts either supported architecture. The modified
        // header fixture is checked and copied only; it must never be executed.
        assert_success(output);
        assert_eq!(fs::read(installed).unwrap(), fs::read(source).unwrap());
    } else {
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("architecture"));
        assert_eq!(fs::read_to_string(installed).unwrap(), "previous version");
    }
    package.assert_no_staging_files(&package.bin_dir());
}

#[test]
fn argument_errors_do_not_create_an_installation() {
    let package = Package::new();
    for script in ["install.sh", "uninstall.sh"] {
        for args in [
            vec!["--unknown"],
            vec!["--bin-dir"],
            vec!["--bin-dir", ""],
            vec!["--bin-dir", "relative/path"],
        ] {
            assert!(!package.run(script, &args).status.success());
        }
        assert_success(package.run(script, &["--help"]));
    }
    for args in [vec!["--binary"], vec!["--binary", ""]] {
        assert!(!package.run("install.sh", &args).status.success());
    }
    assert!(!package.bin_dir().exists());
}

#[test]
fn uninstall_is_idempotent_and_preserves_other_files_and_data() {
    let package = Package::new();
    assert_success(package.run("install.sh", &[]));
    let sentinel_paths = [
        package.bin_dir().join("another-command"),
        package
            .home
            .join("Library/Application Support/xield/geoip/Country.mmdb"),
        package.root.path().join("PF state fixture/network.json"),
    ];
    for path in &sentinel_paths {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "preserved").unwrap();
    }
    assert_success(package.run("uninstall.sh", &[]));
    assert_success(package.run("uninstall.sh", &[]));
    assert!(!package.bin_dir().join("xield").exists());
    for path in &sentinel_paths {
        assert_eq!(fs::read_to_string(path).unwrap(), "preserved");
    }
}

#[test]
fn installation_preserves_extended_attributes() {
    let package = Package::new();
    let attribute = "org.xield.distribution-test";
    assert_success(
        Command::new("/usr/bin/xattr")
            .args(["-w", attribute, "preserved marker"])
            .arg(package.archive.join("xield"))
            .output()
            .unwrap(),
    );
    assert_success(package.run("install.sh", &[]));
    let output = Command::new("/usr/bin/xattr")
        .args(["-p", attribute])
        .arg(package.bin_dir().join("xield"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "preserved marker"
    );
}

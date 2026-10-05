// Evaluation 17 / D14: CLI and Desktop share the same business core. Hashing
// current_exe made identical core builds disagree and invalidated CLI evidence
// when opened in Desktop. Version strings alone miss uncommitted source edits.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
fn collect(root: &Path, path: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
    if path.is_dir() {
        for item in std::fs::read_dir(path).expect("read core inputs") {
            collect(root, &item.expect("core input").path(), files);
        }
    } else {
        let name = path
            .strip_prefix(root)
            .expect("core input inside workspace")
            .to_string_lossy()
            .into_owned();
        files.insert(name, std::fs::read(path).expect("read core input"));
    }
}
fn main() {
    let crate_root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest"));
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("build output"))
            .join("hexagon-firefox-fork.dylib");
        let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("aarch64") => "arm64",
            Ok("x86_64") => "x86_64",
            _ => panic!("unsupported macOS architecture"),
        };
        let status = std::process::Command::new("/usr/bin/clang")
            .args([
                "-dynamiclib",
                "-arch",
                arch,
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Wno-deprecated-declarations",
            ])
            .arg(crate_root.join("src/firefox_fork.c"))
            .arg("-o")
            .arg(output)
            .status()
            .expect("build Firefox fork adapter");
        assert!(status.success(), "Firefox fork adapter compilation failed");
    }
    let root = crate_root
        .parent()
        .and_then(Path::parent)
        .expect("workspace");
    let mut files = BTreeMap::new();
    for path in [
        crate_root.join("src"),
        crate_root.join("migrations"),
        crate_root.join("Cargo.toml"),
        crate_root.join("build.rs"),
        root.join("Cargo.lock"),
        root.join("Cargo.toml"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
        collect(root, &path, &mut files);
    }
    let mut digest = Sha256::new();
    for (name, bytes) in files {
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    let mut env: BTreeMap<_, _> = std::env::vars()
        .filter(|(k, _)| k.starts_with("CARGO_FEATURE_"))
        .collect();
    for key in [
        "TARGET",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        env.insert(key.into(), std::env::var(key).unwrap_or_default());
    }
    let compiler = std::process::Command::new(std::env::var_os("RUSTC").expect("rustc"))
        .args(["--version", "--verbose"])
        .output()
        .expect("compiler identity");
    assert!(compiler.status.success(), "compiler identity unavailable");
    digest.update(&compiler.stdout);
    for (key, value) in env {
        digest.update(key.as_bytes());
        digest.update([0]);
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    println!(
        "cargo:rustc-env=HEXAGON_CORE_BUILD_ID={:x}",
        digest.finalize()
    );
}

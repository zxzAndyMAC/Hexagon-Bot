//! QA16: host-owned fork adapter for Playwright's macOS Firefox launcher.
use std::path::{Path, PathBuf};

const BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hexagon-firefox-fork.dylib"));

pub(super) fn provision(root: &Path) -> std::io::Result<PathBuf> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let root = root.canonicalize()?;
    let admin = root.join(".hexagon");
    std::fs::create_dir_all(&admin)?;
    if admin.canonicalize()? != admin {
        return Err(std::io::Error::other(
            "Firefox administrative directory is redirected",
        ));
    }
    let directory = admin.join("runtime");
    std::fs::create_dir(&directory).or_else(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Ok(())
        } else {
            Err(error)
        }
    })?;
    // Never follow a substituted administrative directory into a host tree.
    if directory.canonicalize()? != directory {
        return Err(std::io::Error::other(
            "Firefox adapter directory is redirected",
        ));
    }
    let path = directory.join(format!("firefox-fork-{:x}.dylib", Sha256::digest(BYTES)));
    if !path.try_exists()? {
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        std::io::Write::write_all(&mut file, BYTES)?;
        file.as_file().sync_all()?;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o444))?;
        match file.persist_noclobber(&path) {
            Ok(_) => (),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.error),
        }
    }
    let metadata = std::fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.nlink() != 1 || std::fs::read(&path)? != BYTES {
        return Err(std::io::Error::other("Firefox adapter identity mismatch"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]
        #[test]
        fn adapter_requires_existing_project_state_and_regular_isolation(owned in any::<bool>(), strict in any::<bool>()) {
            use base64::Engine;
            let root = tempfile::tempdir().unwrap();
            if owned { std::fs::create_dir(root.path().join(".hexagon")).unwrap(); }
            let spec = if strict { crate::sandbox::evaluation_spec(root.path(), &[]) }
                else { crate::sandbox::spec_for(root.path(), &[], false) };
            let crate::sandbox::SandboxSpec::Seatbelt(profile) = spec else { panic!("Seatbelt required"); };
            let spec = crate::sandbox::SandboxSpec::Seatbelt(format!("{profile}\n{}", crate::sandbox::PROCESS_GROUP_RULES));
            let mut command = std::process::Command::new("/bin/sh");
            command.current_dir(root.path());
            let wrapped = crate::sandbox::wrap_command(&mut command, &spec).unwrap();
            let options = wrapped.get_envs().find(|(key, _)| *key == "NODE_OPTIONS").unwrap().1.unwrap().to_str().unwrap();
            let data = options.split_whitespace().next().unwrap().strip_prefix("--import=data:text/javascript;base64,").unwrap();
            let preload = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(data).unwrap()).unwrap();
            prop_assert_eq!(root.path().join(".hexagon").exists(), owned);
            prop_assert_eq!(root.path().join(".hexagon/runtime").exists(), owned && !strict);
            prop_assert_eq!(preload.contains("const firefoxAdapter = null;"), !(owned && !strict));
        }
    }
    #[test]
    fn adapter_is_host_owned_and_substitutions_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let path = provision(root.path()).unwrap();
        assert_eq!(provision(root.path()).unwrap(), path);
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("/dev/null", &path).unwrap();
        assert!(provision(root.path()).is_err());
    }

    #[test]
    fn redirected_administrative_directory_has_no_external_write() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join(".hexagon")).unwrap();
        assert!(provision(root.path()).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn fork_adapter_retains_fd_semantics_and_kernel_boundaries() {
        use std::process::Command;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::fs::create_dir(root.path().join("allowed")).unwrap();
        std::fs::write(root.path().join("allowed/.env"), "SYNTHETIC_QA_SECRET").unwrap();
        std::fs::hard_link(
            root.path().join("allowed/.env"),
            root.path().join("allowed/opaque.data"),
        )
        .unwrap();
        std::os::unix::fs::symlink(".env", root.path().join("allowed/alias.data")).unwrap();
        std::fs::write(root.path().join("probe.c"), include_str!("firefox_probe.c")).unwrap();
        let executable = root.path().join("probe");
        assert!(Command::new("/usr/bin/clang")
            .arg(root.path().join("probe.c"))
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap()
            .success());
        let adapter = provision(root.path()).unwrap();
        let super::super::SandboxSpec::Seatbelt(profile) =
            super::super::spec_for(root.path(), &["allowed/**".into()], false)
        else {
            panic!("Seatbelt missing");
        };
        let spec = super::super::SandboxSpec::Seatbelt(format!(
            "{profile}\n{}",
            super::super::PROCESS_GROUP_RULES
        ));
        let marker = outside.path().join("escaped");
        let mut command = Command::new("/usr/bin/env");
        command
            .current_dir(root.path())
            .arg(format!("DYLD_INSERT_LIBRARIES={}", adapter.display()))
            .arg(format!("QA_OUTSIDE={}", marker.display()))
            .arg(format!("QA_PORT={port}"))
            .arg(&executable);
        let output = super::super::wrap_command(&mut command, &spec)
            .unwrap()
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "FIREFOX_FORK_BOUNDARIES_OK"
        );
        assert!(!marker.exists());
    }

    #[test]
    fn strict_evaluation_keeps_mach_and_network_denied_and_omits_browser_adapter() {
        use base64::Engine;
        use std::process::Command;
        let root = tempfile::tempdir().unwrap();
        let super::super::SandboxSpec::Seatbelt(profile) =
            super::super::evaluation_spec(root.path(), &[])
        else {
            panic!("strict Seatbelt unavailable");
        };
        assert!(profile.contains("(deny mach-register)"));
        assert!(profile.contains("(deny mach-lookup)"));
        assert!(profile.contains("(deny network*)"));
        let spec = super::super::SandboxSpec::Seatbelt(format!(
            "{profile}\n{}",
            super::super::PROCESS_GROUP_RULES
        ));
        let mut command = Command::new("/bin/sh");
        command
            .current_dir(root.path())
            .args(["-c", "nc -z -w1 127.0.0.1 1 2>/dev/null; test $? -ne 0"]);
        let mut wrapped = super::super::wrap_command(&mut command, &spec).unwrap();
        let options = wrapped
            .get_envs()
            .find(|(key, _)| *key == "NODE_OPTIONS")
            .unwrap()
            .1
            .unwrap()
            .to_string_lossy();
        let data = options
            .split("base64,")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        let preload = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap(),
        )
        .unwrap();
        assert!(preload.contains("const firefoxAdapter = null;"));
        assert!(!root.path().join(".hexagon/runtime").exists());
        assert!(
            wrapped.status().unwrap().success(),
            "network allowed in strict evaluation"
        );
    }
}

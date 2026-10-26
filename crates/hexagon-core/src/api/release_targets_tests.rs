//! 发布目标门面（owner-followups-1015 #14）：登记 / 修改 / 删除、凭据录入 / 替换 / 移除，
//! 以及凭据值不进表、轨迹、错误文案（泄漏扫描）。凭据库用内存接缝，不碰系统钥匙串。
use super::*;
use crate::credentials::{leak_scan, CredentialStore, MemoryStore};
use crate::errcode::ErrorCode;
use crate::release_targets::{credential_key, ReleaseTargetDraft};

struct Fixture {
    _dir: tempfile::TempDir,
    wb: Workbench,
    store: Arc<MemoryStore>,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    let store = Arc::new(MemoryStore::default());
    wb.set_credential_store(store.clone());
    Fixture {
        _dir: dir,
        wb,
        store,
    }
}

fn draft(name: &str, command: &str, cwd: &str, creds: &[&str]) -> ReleaseTargetDraft {
    ReleaseTargetDraft {
        id: None,
        name: name.into(),
        command: command.into(),
        cwd: cwd.into(),
        credentials: creds.iter().map(|c| c.to_string()).collect(),
    }
}

fn release_events(wb: &Workbench) -> Vec<serde_json::Value> {
    wb.db
        .conn()
        .prepare("SELECT payload FROM events WHERE project_id=?1 AND kind='system' ORDER BY id")
        .unwrap()
        .query_map([crate::PROJECT_ID], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|raw| serde_json::from_str(&raw.unwrap()).unwrap())
        .filter(|p: &serde_json::Value| {
            p["kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("release_"))
        })
        .collect()
}

#[test]
fn owner_registers_updates_and_removes_a_release_target() {
    let f = fixture();
    assert!(f.wb.release_targets().unwrap().is_empty());

    let npm =
        f.wb.save_release_target(&draft(
            " npm ",
            "npm publish --access public",
            "./packages/web/",
            &["NPM_TOKEN", " NPM_TOKEN", ""],
        ))
        .unwrap();
    assert_eq!(npm.name, "npm");
    assert_eq!(npm.cwd, "packages/web");
    assert_eq!(npm.credentials.len(), 1, "duplicates and blanks collapse");
    assert_eq!(npm.credentials[0].name, "NPM_TOKEN");
    assert!(!npm.credentials[0].set);
    let pages =
        f.wb.save_release_target(&draft("pages", "npm run deploy", "", &[]))
            .unwrap();
    let listed = f.wb.release_targets().unwrap();
    assert_eq!(
        listed.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["npm", "pages"],
        "registration order"
    );

    // 修改：改名、改命令、改凭据列表，id 不变
    let updated =
        f.wb.save_release_target(&ReleaseTargetDraft {
            id: Some(npm.id.clone()),
            ..draft(
                "npm registry",
                "npm publish",
                "",
                &["NPM_TOKEN", "OTP_SEED"],
            )
        })
        .unwrap();
    assert_eq!(updated.id, npm.id);
    assert_eq!(updated.name, "npm registry");
    assert_eq!(updated.command, "npm publish");
    assert_eq!(updated.cwd, "");
    assert_eq!(updated.credentials.len(), 2);

    f.wb.remove_release_target(&pages.id).unwrap();
    let left = f.wb.release_targets().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, npm.id);

    let actions: Vec<String> = release_events(&f.wb)
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                e["kind"].as_str().unwrap(),
                e["action"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        actions,
        [
            "release_target_changed:added",
            "release_target_changed:added",
            "release_target_changed:updated",
            "release_target_changed:removed",
        ]
    );
    for e in release_events(&f.wb) {
        assert!(crate::trace::SYSTEM_SUBKINDS.contains(&e["kind"].as_str().unwrap()));
        assert_eq!(e["by"], "owner");
    }
}

#[test]
fn registration_is_refused_with_reason_codes() {
    let f = fixture();
    f.wb.save_release_target(&draft("npm", "npm publish", "", &[]))
        .unwrap();
    let cases = [
        (draft("  ", "npm publish", "", &[]), "release_name_invalid"),
        (draft("npm", "npm publish", "", &[]), "release_name_taken"),
        (draft("x", "  ", "", &[]), "release_command_invalid"),
        (
            draft("x", "npm publish\nrm -rf /", "", &[]),
            "release_command_invalid",
        ),
        (
            draft("x", "npm publish", "../outside", &[]),
            "release_cwd_outside",
        ),
        (
            draft("x", "npm publish", "/etc", &[]),
            "release_cwd_outside",
        ),
        (
            draft("x", "npm publish", "", &["npm-token"]),
            "release_credential_name_invalid",
        ),
    ];
    for (d, want) in cases {
        let err = f.wb.save_release_target(&d).unwrap_err();
        assert_eq!(err.code(), want, "{d:?}");
    }
    let unknown = ReleaseTargetDraft {
        id: Some("nope".into()),
        ..draft("y", "npm publish", "", &[])
    };
    assert_eq!(
        f.wb.save_release_target(&unknown).unwrap_err().code(),
        "release_target_unknown"
    );
    assert_eq!(
        f.wb.release_targets().unwrap().len(),
        1,
        "nothing half-written"
    );
}

#[test]
fn credentials_are_stored_in_the_keychain_and_never_echoed() {
    let f = fixture();
    let secret = "npm_tok_7f3a9c2e";
    let t =
        f.wb.save_release_target(&draft("npm", "npm publish", "", &["NPM_TOKEN"]))
            .unwrap();

    let after =
        f.wb.set_release_credential(&t.id, "NPM_TOKEN", &format!("{secret}\n"))
            .unwrap();
    assert!(after.credentials[0].set);
    assert_eq!(
        f.store
            .get(&credential_key(&t.id, "NPM_TOKEN"))
            .unwrap()
            .as_deref(),
        Some(secret),
        "pasted trailing newline is not part of the value"
    );
    // 读面没有值
    let shown = serde_json::to_string(&f.wb.release_targets().unwrap()).unwrap();
    assert!(!shown.contains(secret));

    // 替换
    f.wb.set_release_credential(&t.id, "NPM_TOKEN", "npm_tok_second")
        .unwrap();
    assert_eq!(
        f.store
            .get(&credential_key(&t.id, "NPM_TOKEN"))
            .unwrap()
            .as_deref(),
        Some("npm_tok_second")
    );

    // 移除：名字还在，显示未设置
    let removed = f.wb.remove_release_credential(&t.id, "NPM_TOKEN").unwrap();
    assert_eq!(removed.credentials[0].name, "NPM_TOKEN");
    assert!(!removed.credentials[0].set);
    assert!(f
        .store
        .get(&credential_key(&t.id, "NPM_TOKEN"))
        .unwrap()
        .is_none());

    let actions: Vec<String> = release_events(&f.wb)
        .iter()
        .filter(|e| e["kind"] == "release_credential_changed")
        .map(|e| e["action"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(actions, ["set", "replaced", "removed"]);

    // 值不进事件 / 消息 / 卡片 / 发布目标表
    for value in [secret, "npm_tok_second"] {
        let hits = leak_scan(&f.wb.db, crate::PROJECT_ID, value).unwrap();
        assert!(hits.is_empty(), "leak at {hits:?}");
    }
}

#[test]
fn credential_input_is_refused_with_reason_codes() {
    let f = fixture();
    let t =
        f.wb.save_release_target(&draft("npm", "npm publish --tag next", "", &["NPM_TOKEN"]))
            .unwrap();
    let cases = [
        ("OTHER", "x-secret", "release_credential_undeclared"),
        ("NPM_TOKEN", "  \n", "release_secret_empty"),
        // 值出现在命令里：录入即拒，否则命令原文会把令牌带进卡片与轨迹
        ("NPM_TOKEN", "next", "release_secret_in_target"),
    ];
    for (name, value, want) in cases {
        let err = f.wb.set_release_credential(&t.id, name, value).unwrap_err();
        assert_eq!(err.code(), want);
        assert!(!err.to_string().contains(value.trim()) || value.trim().is_empty());
    }
    assert_eq!(
        f.wb.set_release_credential("nope", "NPM_TOKEN", "v")
            .unwrap_err()
            .code(),
        "release_target_unknown"
    );
    assert!(!f.wb.release_targets().unwrap()[0].credentials[0].set);
}

#[test]
fn pasting_a_stored_secret_into_the_command_is_refused() {
    let f = fixture();
    let t =
        f.wb.save_release_target(&draft("npm", "npm publish", "", &["NPM_TOKEN"]))
            .unwrap();
    f.wb.set_release_credential(&t.id, "NPM_TOKEN", "npm_tok_live_42")
        .unwrap();
    let err =
        f.wb.save_release_target(&ReleaseTargetDraft {
            id: Some(t.id.clone()),
            ..draft(
                "npm",
                "NPM_TOKEN=npm_tok_live_42 npm publish",
                "",
                &["NPM_TOKEN"],
            )
        })
        .unwrap_err();
    assert_eq!(err.code(), "release_secret_in_target");
    assert!(!err.to_string().contains("npm_tok_live_42"));
    assert_eq!(f.wb.release_targets().unwrap()[0].command, "npm publish");
    assert!(leak_scan(&f.wb.db, crate::PROJECT_ID, "npm_tok_live_42")
        .unwrap()
        .is_empty());
}

#[test]
fn dropping_a_credential_or_the_target_deletes_its_keychain_entries() {
    let f = fixture();
    let t =
        f.wb.save_release_target(&draft("npm", "npm publish", "", &["NPM_TOKEN", "OTP_SEED"]))
            .unwrap();
    f.wb.set_release_credential(&t.id, "NPM_TOKEN", "a-1")
        .unwrap();
    f.wb.set_release_credential(&t.id, "OTP_SEED", "b-2")
        .unwrap();

    // 从列表里拿掉 OTP_SEED：钥匙串条目一起删，不留看不见的孤儿秘密
    f.wb.save_release_target(&ReleaseTargetDraft {
        id: Some(t.id.clone()),
        ..draft("npm", "npm publish", "", &["NPM_TOKEN"])
    })
    .unwrap();
    assert!(f
        .store
        .get(&credential_key(&t.id, "OTP_SEED"))
        .unwrap()
        .is_none());
    assert!(f
        .store
        .get(&credential_key(&t.id, "NPM_TOKEN"))
        .unwrap()
        .is_some());

    f.wb.remove_release_target(&t.id).unwrap();
    assert!(f
        .store
        .get(&credential_key(&t.id, "NPM_TOKEN"))
        .unwrap()
        .is_none());
    assert_eq!(
        f.wb.remove_release_target(&t.id).unwrap_err().code(),
        "release_target_unknown"
    );
}

#[test]
fn leak_scan_flags_a_secret_inside_a_release_target() {
    let f = fixture();
    // 凭据名之外的位置（这里是命令）出现了疑似值：泄漏扫描按 release_targets:<id> 报出
    let t =
        f.wb.save_release_target(&draft("npm", "npm publish --token tok-abc-123", "", &[]))
            .unwrap();
    assert_eq!(
        leak_scan(&f.wb.db, crate::PROJECT_ID, "tok-abc-123").unwrap(),
        vec![format!("release_targets:{}", t.id)]
    );
}

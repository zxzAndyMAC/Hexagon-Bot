use super::*;
use proptest::prelude::*;
use std::fs;
use std::sync::Mutex;

/// 假目录：handlers 按「文件 / 目录」各给一份固定答复；installed 查表；launch 只记账。
#[derive(Default)]
struct Fake {
    file_handlers: Vec<InstalledApp>,
    dir_handlers: Vec<InstalledApp>,
    installed: Vec<InstalledApp>,
    launched: Mutex<Vec<(String, PathBuf)>>,
    fail_launch: bool,
}

fn app(id: &str, name: &str) -> InstalledApp {
    InstalledApp {
        bundle_id: id.into(),
        name: name.into(),
        app_path: PathBuf::from(format!("/Apps/{name}.app")),
    }
}

impl AppCatalog for Fake {
    fn handlers(&self, target: &Path) -> Vec<InstalledApp> {
        if target.is_dir() {
            self.dir_handlers.clone()
        } else {
            self.file_handlers.clone()
        }
    }
    fn installed(&self, id: &str) -> Option<InstalledApp> {
        self.installed.iter().find(|a| a.bundle_id == id).cloned()
    }
    fn icon_data_url(&self, a: &InstalledApp) -> Option<String> {
        (a.bundle_id != "dev.zed.Zed").then(|| format!("data:image/png;base64,{}", a.name))
    }
    fn launch(&self, a: &InstalledApp, target: &Path) -> Result<(), String> {
        if self.fail_launch {
            return Err("boom".into());
        }
        self.launched
            .lock()
            .unwrap()
            .push((a.bundle_id.clone(), target.to_path_buf()));
        Ok(())
    }
}

const VSCODE: &str = "com.microsoft.VSCode";
const COT: &str = "com.coteditor.CotEditor";
const ZED: &str = "dev.zed.Zed";

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
    fs::create_dir_all(tmp.path().join("outside")).unwrap();
    fs::write(tmp.path().join("outside/secret"), "x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.path().join("outside"), root.join("link_out")).unwrap();
    (tmp, root)
}

fn fake() -> Fake {
    Fake {
        // 系统说能开这个文件的：VS Code、CotEditor、预览、终端（后两者不在白名单）
        file_handlers: vec![
            app(VSCODE, "Visual Studio Code"),
            app(COT, "CotEditor"),
            app("com.apple.Preview", "Preview"),
            app("com.apple.Terminal", "Terminal"),
            app(VSCODE, "Visual Studio Code"),
        ],
        // 系统说能开目录的：访达（不在白名单）
        dir_handlers: vec![app("com.apple.finder", "Finder")],
        installed: vec![
            app(VSCODE, "Visual Studio Code"),
            app(COT, "CotEditor"),
            app(ZED, "Zed"),
            app("com.apple.Terminal", "Terminal"),
        ],
        ..Fake::default()
    }
}

fn ids(list: &[OpenWithApp]) -> Vec<&str> {
    list.iter().map(|a| a.id.as_str()).collect()
}

#[test]
fn file_candidates_are_system_handlers_intersected_with_allowlist() {
    let (_t, root) = fixture();
    let list = candidates(&fake(), &root, "src/main.rs").unwrap();
    // 去重、按显示名排序；预览 / 终端不在白名单不列；Zed 已装但系统没登记它能开这个文件 → 不列
    assert_eq!(ids(&list), vec![COT, VSCODE]);
    assert_eq!(list[1].name, "Visual Studio Code");
    assert_eq!(
        list[1].icon.as_deref(),
        Some("data:image/png;base64,Visual Studio Code")
    );
}

#[test]
fn dir_candidates_only_list_apps_that_open_folders() {
    let (_t, root) = fixture();
    let f = fake();
    // 项目根（空串）与子目录同口径：CotEditor 不开目录、终端不在白名单、访达不在白名单
    for rel in ["", "src", "./src/"] {
        let list = candidates(&f, &root, rel).unwrap();
        assert_eq!(ids(&list), vec![VSCODE, ZED], "{rel:?}");
        // 取不到图标 → null（界面中性图标）
        assert_eq!(list[1].icon, None);
    }
}

#[test]
fn dir_handler_without_opens_dirs_flag_is_listed_when_system_says_so() {
    let (_t, root) = fixture();
    let mut f = fake();
    f.dir_handlers.push(app(COT, "CotEditor"));
    assert_eq!(
        ids(&candidates(&f, &root, "src").unwrap()),
        vec![COT, VSCODE, ZED]
    );
}

#[test]
fn catalog_answering_with_another_bundle_is_not_trusted() {
    let (_t, root) = fixture();
    struct Liar;
    impl AppCatalog for Liar {
        fn handlers(&self, _: &Path) -> Vec<InstalledApp> {
            Vec::new()
        }
        fn installed(&self, _: &str) -> Option<InstalledApp> {
            Some(app("com.apple.Terminal", "Terminal"))
        }
        fn icon_data_url(&self, _: &InstalledApp) -> Option<String> {
            None
        }
        fn launch(&self, _: &InstalledApp, _: &Path) -> Result<(), String> {
            panic!("must not launch")
        }
    }
    assert!(candidates(&Liar, &root, "").unwrap().is_empty());
    assert_eq!(
        crate::errcode::variant_code(&open_with(&Liar, &root, "", VSCODE).unwrap_err()),
        "open_with_unsupported"
    );
}

#[test]
fn out_of_root_and_missing_paths_are_refused() {
    let (_t, root) = fixture();
    let f = fake();
    for rel in ["..", "../outside/secret", "/etc/passwd", "C:\\x", "a\0b"] {
        let e = candidates(&f, &root, rel).unwrap_err();
        assert_eq!(
            crate::errcode::variant_code(&e),
            "open_with_outside_project",
            "{rel:?}"
        );
        let e = open_with(&f, &root, rel, VSCODE).unwrap_err();
        assert_eq!(
            crate::errcode::variant_code(&e),
            "open_with_outside_project",
            "{rel:?}"
        );
    }
    #[cfg(unix)]
    assert_eq!(
        crate::errcode::variant_code(&open_with(&f, &root, "link_out/secret", VSCODE).unwrap_err()),
        "open_with_outside_project"
    );
    assert_eq!(
        crate::errcode::variant_code(&candidates(&f, &root, "nope.rs").unwrap_err()),
        "open_with_missing"
    );
    assert!(f.launched.lock().unwrap().is_empty());
}

#[test]
fn open_with_launches_only_current_candidates_with_canonical_target() {
    let (_t, root) = fixture();
    let f = fake();
    let base = fs::canonicalize(&root).unwrap();
    open_with(&f, &root, "src/main.rs", VSCODE).unwrap();
    open_with(&f, &root, "", ZED).unwrap();
    assert_eq!(
        *f.launched.lock().unwrap(),
        vec![
            (VSCODE.to_string(), base.join("src/main.rs")),
            (ZED.to_string(), base.clone())
        ]
    );
    // 不在白名单（终端已装、系统也说能开）· 不开目录的应用开目录 · 没登记能开该文件的已装应用
    for (rel, id) in [
        ("src/main.rs", "com.apple.Terminal"),
        ("src", COT),
        ("src/main.rs", ZED),
        ("src/main.rs", "; rm -rf /"),
    ] {
        let e = open_with(&f, &root, rel, id).unwrap_err();
        assert_eq!(
            crate::errcode::variant_code(&e),
            "open_with_unsupported",
            "{rel} {id}"
        );
    }
    assert_eq!(f.launched.lock().unwrap().len(), 2);
}

#[test]
fn launch_failure_maps_to_launch_code() {
    let (_t, root) = fixture();
    let f = Fake {
        fail_launch: true,
        ..fake()
    };
    assert_eq!(
        crate::errcode::variant_code(&open_with(&f, &root, "src/main.rs", VSCODE).unwrap_err()),
        "open_with_launch"
    );
}

#[test]
fn refusal_writes_a_warn_reject_record() {
    let _g = crate::diag::TEST_LEVEL_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    log::set_max_level(log::LevelFilter::Warn);
    let (_t, root) = fixture();
    let _ = open_with(&fake(), &root, "../outside", VSCODE);
    let rs = crate::diag::records(Some(crate::PROJECT_ID), Some(crate::diag::CLASS_REJECT));
    let hit = rs
        .iter()
        .rev()
        .find(|r| r.branch == "open_with")
        .expect("reject record");
    assert_eq!(hit.level, "warn");
    assert_eq!(hit.code, "open_with_outside_project");
}

#[test]
fn allowlist_ids_are_unique() {
    let mut seen = HashSet::new();
    for e in EDITORS {
        assert!(seen.insert(e.bundle_id), "duplicate {}", e.bundle_id);
    }
}

/// 手动核对（#31）：列出本机探测到的编辑器，不启动任何应用。
/// `cargo test -p hexagon-core open_with::tests::list_this_machine -- --ignored --nocapture`
#[test]
#[ignore]
fn list_this_machine() {
    let here = std::env::current_dir().unwrap();
    let root = here.ancestors().nth(2).unwrap_or(&here).to_path_buf();
    for rel in [
        "",
        "Cargo.toml",
        "crates/hexagon-core/src/lib.rs",
        "AGENTS.md",
    ] {
        let list = candidates(system_catalog(), &root, rel).unwrap();
        println!(
            "{rel:?}: {:?}",
            list.iter()
                .map(|a| format!(
                    "{} ({}) icon={}",
                    a.name,
                    a.id,
                    a.icon.as_ref().map_or(0, |s| s.len())
                ))
                .collect::<Vec<_>>()
        );
        if let Some(icon) = list.first().and_then(|a| a.icon.as_deref()) {
            if let Ok(dir) = std::env::var("OPEN_WITH_ICON_DUMP") {
                use base64::Engine;
                let b64 = icon.trim_start_matches("data:image/png;base64,");
                let png = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap();
                fs::write(Path::new(&dir).join("first.png"), png).unwrap();
            }
        }
    }
}

fn any_app() -> impl Strategy<Value = InstalledApp> {
    let ids = vec![
        VSCODE,
        COT,
        ZED,
        "com.apple.Terminal",
        "com.apple.finder",
        "x.evil",
    ];
    (prop::sample::select(ids), "[A-Za-z ]{1,8}").prop_map(|(id, name)| app(id, &name))
}

proptest! {
    // D12：判定面属性测试。不变量 = 列出的每一项都在白名单、且（文件）系统登记能开 /
    // （目录）系统登记能开或已装且标了 opens_dirs；open_with 放行 ⇔ 在候选里。
    #[test]
    fn candidates_respect_allowlist_and_kind(
        file_h in prop::collection::vec(any_app(), 0..5),
        dir_h in prop::collection::vec(any_app(), 0..5),
        inst in prop::collection::vec(any_app(), 0..5),
        rel in prop::sample::select(vec!["", "src", "src/main.rs"]),
        pick in prop::sample::select(vec![VSCODE, COT, ZED, "com.apple.Terminal", "x.evil"]),
    ) {
        let (_t, root) = fixture();
        let f = Fake { file_handlers: file_h.clone(), dir_handlers: dir_h.clone(), installed: inst.clone(), ..Fake::default() };
        let is_dir = rel != "src/main.rs";
        let list = candidates(&f, &root, rel).unwrap();
        let mut uniq = HashSet::new();
        for a in &list {
            prop_assert!(uniq.insert(a.id.clone()), "dup {}", a.id);
            let entry = editor(&a.id);
            prop_assert!(entry.is_some(), "not allow-listed {}", a.id);
            let by_system = if is_dir { &dir_h } else { &file_h }.iter().any(|h| h.bundle_id == a.id);
            let by_flag = is_dir && entry.unwrap().opens_dirs && inst.iter().any(|h| h.bundle_id == a.id);
            prop_assert!(by_system || by_flag, "{} not justified", a.id);
        }
        let allowed = open_with(&f, &root, rel, pick).is_ok();
        prop_assert_eq!(allowed, list.iter().any(|a| a.id == pick));
    }
}

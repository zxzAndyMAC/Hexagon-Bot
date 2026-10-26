use super::*;
use proptest::prelude::*;
use std::fs;
use std::sync::Mutex;

/// 假目录：handlers 按「文件 / 目录」各给一份固定答复，`by_ext` 按扩展名覆盖（#32 按类型给系统列表）；
/// UTI 按扩展名查 [`UTI`]、符合关系查 [`PARENTS`]（传递闭包）；installed 查表；launch 只记账。
#[derive(Default)]
struct Fake {
    file_handlers: Vec<InstalledApp>,
    dir_handlers: Vec<InstalledApp>,
    by_ext: Vec<(&'static str, Vec<InstalledApp>)>,
    /// 系统默认应用（bundle id），不管类型。
    default: Option<&'static str>,
    installed: Vec<InstalledApp>,
    launched: Mutex<Vec<(String, PathBuf)>>,
    fail_launch: bool,
}

/// 扩展名 → UTI（假的 LaunchServices；不在表里 = 系统给出 `dyn.*`）。
const UTI: &[(&str, &str)] = &[
    ("rs", "public.rust-source"),
    ("md", "net.daringfireball.markdown"),
    ("sh", "public.shell-script"),
    ("pdf", "com.adobe.pdf"),
    ("pptx", "org.openxmlformats.presentationml.presentation"),
    ("png", "public.png"),
    ("key", "com.apple.keynote.key"),
    ("app", "com.apple.application-bundle"),
];

/// UTI 直接父类型（真系统的子集）。
const PARENTS: &[(&str, &str)] = &[
    ("public.rust-source", "public.source-code"),
    ("public.shell-script", "public.script"),
    ("public.script", "public.source-code"),
    ("public.source-code", "public.plain-text"),
    ("net.daringfireball.markdown", "public.plain-text"),
    ("public.plain-text", "public.text"),
    ("public.text", "public.data"),
    ("com.adobe.pdf", "public.data"),
    (
        "org.openxmlformats.presentationml.presentation",
        "public.presentation",
    ),
    ("public.png", "public.image"),
    ("public.image", "public.data"),
    ("com.apple.keynote.key", "com.apple.package"),
    ("com.apple.application-bundle", "com.apple.application"),
    ("com.apple.application-bundle", "com.apple.package"),
    ("com.apple.package", "public.directory"),
    ("public.folder", "public.directory"),
];

fn ext_of(target: &Path) -> &str {
    target.extension().and_then(|e| e.to_str()).unwrap_or("")
}

fn fake_conforms(ty: &str, to: &str) -> bool {
    ty == to
        || PARENTS
            .iter()
            .any(|(c, p)| *c == ty && fake_conforms(p, to))
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
        if let Some((_, hs)) = self.by_ext.iter().find(|(e, _)| *e == ext_of(target)) {
            return hs.clone();
        }
        if target.is_dir() {
            self.dir_handlers.clone()
        } else {
            self.file_handlers.clone()
        }
    }
    fn installed(&self, id: &str) -> Option<InstalledApp> {
        self.installed.iter().find(|a| a.bundle_id == id).cloned()
    }
    fn default_handler(&self, target: &Path) -> Option<InstalledApp> {
        let id = self.default?;
        self.handlers(target)
            .into_iter()
            .chain(self.installed.iter().cloned())
            .find(|a| a.bundle_id == id)
            .or_else(|| Some(app(id, id)))
    }
    fn type_of(&self, target: &Path) -> Option<String> {
        if let Some((_, u)) = UTI.iter().find(|(e, _)| *e == ext_of(target)) {
            return Some((*u).to_string());
        }
        Some(if target.is_dir() {
            "public.folder".into()
        } else {
            "dyn.ah62d4rv4ge8".into()
        })
    }
    fn conforms(&self, ty: &str, to: &str) -> bool {
        fake_conforms(ty, to)
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
    for f in [
        "doc.pdf",
        "deck.pptx",
        "pic.png",
        "README.md",
        "blob.zzz",
        "run.sh",
    ] {
        fs::write(root.join(f), "x").unwrap();
    }
    fs::create_dir_all(root.join("talk.key/Data")).unwrap();
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
        fn default_handler(&self, _: &Path) -> Option<InstalledApp> {
            Some(app("com.apple.Terminal", "Terminal"))
        }
        fn type_of(&self, _: &Path) -> Option<String> {
            Some("public.folder".into())
        }
        fn conforms(&self, a: &str, b: &str) -> bool {
            a == b
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

const PREVIEW: &str = "com.apple.Preview";
const KEYNOTE: &str = "com.apple.iWork.Keynote";
const PPT: &str = "com.microsoft.Powerpoint";
const CHROME: &str = "com.google.Chrome";

/// #32 类型口径的假系统：pdf / pptx / png / key 各给一份系统登记列表（含重复、终端、IDE）。
fn typed_fake() -> Fake {
    Fake {
        by_ext: vec![
            (
                "pdf",
                vec![
                    app(CHROME, "Google Chrome"),
                    app(PREVIEW, "Preview"),
                    app(PREVIEW, "Preview"),
                    app("com.apple.Terminal", "Terminal"),
                ],
            ),
            (
                "pptx",
                vec![app(PPT, "Microsoft PowerPoint"), app(KEYNOTE, "Keynote")],
            ),
            // 系统自己登记了 VS Code 能开 png → 照列（不额外补，也不剔）
            (
                "png",
                vec![app(VSCODE, "Visual Studio Code"), app(PREVIEW, "Preview")],
            ),
            ("key", vec![app(KEYNOTE, "Keynote")]),
            (
                "zzz",
                vec![app("com.apple.TextEdit", "TextEdit"), app(COT, "CotEditor")],
            ),
        ],
        default: Some(PREVIEW),
        ..fake()
    }
}

#[test]
fn candidates_follow_file_type() {
    let (_t, root) = fixture();
    let f = typed_fake();
    // (路径, 分类, 期望 id 序, 默认标在第几行)
    let rows: &[(&str, TargetClass, &[&str], Option<usize>)] = &[
        // 其余：系统列表、默认在前、去重、剔终端、不补 IDE
        ("doc.pdf", TargetClass::Other, &[PREVIEW, CHROME], Some(0)),
        // 默认应用不在系统给这个类型的列表里 → 不标、不硬塞
        ("deck.pptx", TargetClass::Other, &[KEYNOTE, PPT], None),
        ("pic.png", TargetClass::Other, &[PREVIEW, VSCODE], Some(0)),
        // 文档包在磁盘上是目录，按其余类
        ("talk.key", TargetClass::Other, &[KEYNOTE], None),
        // 未知类型（dyn.*）→ 系统列表
        (
            "blob.zzz",
            TargetClass::Other,
            &[COT, "com.apple.TextEdit"],
            None,
        ),
        // 文本 / 代码：白名单 ∩ 系统登记（#31 口径不变，无默认标）
        ("src/main.rs", TargetClass::Text, &[COT, VSCODE], None),
        ("README.md", TargetClass::Text, &[COT, VSCODE], None),
        ("run.sh", TargetClass::Text, &[COT, VSCODE], None),
        ("src", TargetClass::Folder, &[VSCODE, ZED], None),
        ("", TargetClass::Folder, &[VSCODE, ZED], None),
    ];
    for (rel, class, want, default_at) in rows {
        let target = resolve(&root, rel).unwrap();
        assert_eq!(classify(&f, &target), *class, "{rel:?}");
        let list = candidates(&f, &root, rel).unwrap();
        assert_eq!(ids(&list), *want, "{rel:?}");
        let defaults: Vec<usize> = (0..list.len()).filter(|&i| list[i].is_default).collect();
        assert_eq!(
            defaults,
            default_at.iter().copied().collect::<Vec<_>>(),
            "{rel:?}"
        );
    }
    // 启动时同一规则重算：pdf 能交给预览；终端（系统登记了但被剔）与没登记的 IDE 都拒
    open_with(&f, &root, "doc.pdf", PREVIEW).unwrap();
    for id in ["com.apple.Terminal", VSCODE, ZED] {
        assert_eq!(
            crate::errcode::variant_code(&open_with(&f, &root, "doc.pdf", id).unwrap_err()),
            "open_with_unsupported",
            "{id}"
        );
    }
}

#[test]
fn system_list_is_capped_and_keeps_the_default() {
    let (_t, root) = fixture();
    let many: Vec<InstalledApp> = (0..30)
        .map(|i| app(&format!("x.app{i:02}"), &format!("App {i:02}")))
        .collect();
    let f = Fake {
        by_ext: vec![("pdf", many)],
        default: Some("x.app29"),
        ..fake()
    };
    let list = candidates(&f, &root, "doc.pdf").unwrap();
    assert_eq!(list.len(), MAX_SYSTEM_APPS);
    assert_eq!(list[0].id, "x.app29");
    assert!(list[0].is_default);
}

#[test]
fn recent_project_root_comes_from_the_recents_record() {
    let (tmp, root) = fixture();
    let f = fake();
    let known = vec![root.clone()];
    let dir = root.to_string_lossy().to_string();
    assert_eq!(
        ids(&recent_candidates(&f, &known, &dir).unwrap()),
        vec![VSCODE, ZED]
    );
    recent_open_with(&f, &known, &dir, ZED).unwrap();
    // 不在记录里的目录（哪怕真实存在）、记录里项目根下的子目录、尾斜杠变形：一律拒，不启动
    let outside = tmp.path().join("outside").to_string_lossy().to_string();
    let sub = root.join("src").to_string_lossy().to_string();
    for d in [outside.as_str(), sub.as_str(), &format!("{dir}/"), "/", ""] {
        assert_eq!(
            crate::errcode::variant_code(&recent_candidates(&f, &known, d).unwrap_err()),
            "open_with_outside_project",
            "{d:?}"
        );
        assert!(recent_open_with(&f, &known, d, ZED).is_err(), "{d:?}");
    }
    // 记录里的项目已删 → 缺失
    let gone = vec![tmp.path().join("gone")];
    let gone_dir = gone[0].to_string_lossy().to_string();
    assert_eq!(
        crate::errcode::variant_code(&recent_candidates(&f, &gone, &gone_dir).unwrap_err()),
        "open_with_missing"
    );
    // 白名单闸照旧：CotEditor 不开目录
    assert!(recent_open_with(&f, &known, &dir, COT).is_err());
    assert_eq!(f.launched.lock().unwrap().len(), 1);
}

#[test]
fn allowlist_ids_are_unique() {
    let mut seen = HashSet::new();
    for e in EDITORS {
        assert!(seen.insert(e.bundle_id), "duplicate {}", e.bundle_id);
    }
    // #32 负责人 2026-10-21 定论：ZCode 不进白名单
    assert!(editor("dev.zcode.app").is_none());
}

/// 手动核对（#31；#32 补 .pdf / .pptx）：列出本机探测到的候选，不启动任何应用。
/// `cargo test -p hexagon-core open_with::tests::list_this_machine -- --ignored --nocapture`
/// pdf / pptx 用临时目录里的空文件（系统按扩展名给 UTI，不读内容）。
#[test]
#[ignore]
fn list_this_machine() {
    let here = std::env::current_dir().unwrap();
    let root = here.ancestors().nth(2).unwrap_or(&here).to_path_buf();
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("sample.pdf"), "").unwrap();
    fs::write(tmp.path().join("sample.pptx"), "").unwrap();
    let cases: Vec<(&Path, &str)> = vec![
        (&root, ""),
        (&root, "Cargo.toml"),
        (&root, "crates/hexagon-core/src/lib.rs"),
        (&root, "AGENTS.md"),
        (tmp.path(), "sample.pdf"),
        (tmp.path(), "sample.pptx"),
    ];
    for (root, rel) in cases {
        let target = resolve(root, rel).unwrap();
        let class = classify(system_catalog(), &target);
        let ty = system_catalog().type_of(&target);
        let list = candidates(system_catalog(), root, rel).unwrap();
        println!(
            "{rel:?} [{class:?} {ty:?}]: {:?}",
            list.iter()
                .map(|a| format!(
                    "{}{} ({}) icon={}",
                    a.name,
                    if a.is_default { " [default]" } else { "" },
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
        PREVIEW,
        "com.apple.Terminal",
        "com.apple.finder",
        "x.evil",
    ];
    (prop::sample::select(ids), "[A-Za-z ]{1,8}").prop_map(|(id, name)| app(id, &name))
}

proptest! {
    // D12：判定面属性测试。不变量 = 不出重复；文本 / 目录类每一项都在白名单、且（文件）系统登记能开 /
    // （目录）系统登记能开或已装且标了 opens_dirs；其余类（#32）每一项都是系统对这个类型报过的、不在
    // LAUNCHER_DENY、不超上限、默认最多一个且在第一行；open_with 放行 ⇔ 在候选里。
    #[test]
    fn candidates_respect_allowlist_and_kind(
        file_h in prop::collection::vec(any_app(), 0..5),
        dir_h in prop::collection::vec(any_app(), 0..5),
        typed_h in prop::collection::vec(any_app(), 0..16),
        inst in prop::collection::vec(any_app(), 0..5),
        default in prop::option::of(prop::sample::select(vec![VSCODE, PREVIEW, "com.apple.Terminal", "x.evil"])),
        rel in prop::sample::select(vec!["", "src", "src/main.rs", "README.md", "doc.pdf", "pic.png", "blob.zzz", "talk.key"]),
        pick in prop::sample::select(vec![VSCODE, COT, ZED, PREVIEW, "com.apple.Terminal", "x.evil"]),
    ) {
        let (_t, root) = fixture();
        let f = Fake {
            file_handlers: file_h.clone(),
            dir_handlers: dir_h.clone(),
            by_ext: ["pdf", "png", "zzz", "key"].into_iter().map(|e| (e, typed_h.clone())).collect(),
            default,
            installed: inst.clone(),
            ..Fake::default()
        };
        let list = candidates(&f, &root, rel).unwrap();
        let mut uniq = HashSet::new();
        for a in &list {
            prop_assert!(uniq.insert(a.id.clone()), "dup {}", a.id);
        }
        match classify(&f, &resolve(&root, rel).unwrap()) {
            TargetClass::Other => {
                prop_assert!(list.len() <= MAX_SYSTEM_APPS);
                for (i, a) in list.iter().enumerate() {
                    prop_assert!(typed_h.iter().any(|h| h.bundle_id == a.id), "{} not reported for {rel}", a.id);
                    prop_assert!(!LAUNCHER_DENY.contains(&a.id.as_str()), "launcher {}", a.id);
                    if a.is_default {
                        prop_assert_eq!(i, 0);
                        prop_assert_eq!(Some(a.id.as_str()), default);
                    }
                }
            }
            class => {
                let is_dir = class == TargetClass::Folder;
                for a in &list {
                    prop_assert!(!a.is_default);
                    let entry = editor(&a.id);
                    prop_assert!(entry.is_some(), "not allow-listed {}", a.id);
                    let by_system = if is_dir { &dir_h } else { &file_h }.iter().any(|h| h.bundle_id == a.id);
                    let by_flag = is_dir && entry.unwrap().opens_dirs && inst.iter().any(|h| h.bundle_id == a.id);
                    prop_assert!(by_system || by_flag, "{} not justified", a.id);
                }
            }
        }
        let allowed = open_with(&f, &root, rel, pick).is_ok();
        prop_assert_eq!(allowed, list.iter().any(|a| a.id == pick));
    }
}

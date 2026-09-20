//! Tauri 壳：command 只做转发，业务全在 hexagon-core 的 Workbench。
//! UI 的唯一通道是这些 command + 后续的事件推送 channel，没有旁路。

use hexagon_core::api::Workbench;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

struct AppState {
    wb: Mutex<Option<Workbench>>,
    /// 项目库路径（turn-streaming 票 04/05）：回合进行中 wb 锁被占，
    /// owner 消息/暂停指令经第二条 Db 连接旁路落库，不然干预进不来。
    db_path: Mutex<Option<std::path::PathBuf>>,
}

/// 回合 delta → webview（turn-streaming 票 03）：所有 Workbench 构造点
/// 统一挂 emit。delta 是瞬时展示通道，持久层照旧走 events/messages。
fn attach_delta_hook(app: &tauri::AppHandle, wb: &Workbench) {
    let h = app.clone();
    wb.set_turn_delta_hook(Some(Box::new(move |d| {
        let _ = h.emit("turn-delta", d);
    })));
}

/// 旁路 Db：回合进行中也能写（WAL 双连接 + busy_timeout 兜底）。
fn side_db(state: &AppState) -> Result<Option<hexagon_core::db::Db>, String> {
    let p = state.db_path.lock().map_err(|e| e.to_string())?.clone();
    p.map(|p| hexagon_core::db::Db::open(p).map_err(|e| e.to_string()))
        .transpose()
}

fn with_wb<R>(
    state: &AppState,
    f: impl FnOnce(&Workbench) -> Result<R, hexagon_core::api::ApiError>,
) -> Result<R, String> {
    let g = state.wb.lock().map_err(|e| e.to_string())?;
    let wb = g.as_ref().ok_or_else(|| "no project open".to_string())?;
    f(wb).map_err(|e| e.to_string())
}

fn with_wb_mut<R>(
    state: &AppState,
    f: impl FnOnce(&mut Workbench) -> Result<R, hexagon_core::api::ApiError>,
) -> Result<R, String> {
    let mut g = state.wb.lock().map_err(|e| e.to_string())?;
    let wb = g.as_mut().ok_or_else(|| "no project open".to_string())?;
    f(wb).map_err(|e| e.to_string())
}

#[tauri::command]
fn core_ping() -> String {
    hexagon_core::ping()
}

#[tauri::command]
fn open_project(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    dir: String,
    name: String,
    roles: Vec<(String, String)>,
    pack_json: Option<String>,
) -> Result<(), String> {
    let pack = pack_json
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e: serde_json::Error| e.to_string())?;
    let mut wb = Workbench::open(&dir, &name, &roles, pack).map_err(|e| e.to_string())?;
    hexagon_core::providers::register_all(
        &mut wb.providers,
        Arc::new(hexagon_core::credentials::OsKeychain),
    );
    attach_delta_hook(&app, &wb);
    *state.db_path.lock().map_err(|e| e.to_string())? =
        Some(std::path::Path::new(&dir).join(".hexagon/state.db"));
    *state.wb.lock().map_err(|e| e.to_string())? = Some(wb);
    remember_recent(&app, &dir, &name, "pack");
    Ok(())
}

#[tauri::command]
fn timeline(
    state: tauri::State<AppState>,
    after: Option<i64>,
    limit: usize,
) -> Result<Value, String> {
    with_wb(&state, |wb| {
        wb.timeline(after, limit)
            .map(|v| serde_json::to_value(v).unwrap())
    })
}

#[tauri::command]
fn send_message(state: tauri::State<AppState>, body: String) -> Result<i64, String> {
    // 票 04/05：回合进行中 wb 锁被 dispatch 占着——消息和 暂停/恢复
    // 指令必须走旁路 Db 落库，否则 steering 和流中叫停永远排在回合后面。
    // 其余指令（rewind/stamp/skip/override/install）仍排 wb 队列——
    // 回合中途本来也不该执行它们。
    if let Some(db) = side_db(&state)? {
        let (id, cmd) =
            hexagon_core::commands::send_message_side(&db, hexagon_core::PROJECT_ID, &body)
                .map_err(|e| e.to_string())?;
        use hexagon_core::commands::TextCommand;
        match cmd {
            Some(TextCommand::Pause) => {
                hexagon_core::orchestra::pause(&db, hexagon_core::PROJECT_ID)
                    .map_err(|e| e.to_string())?;
            }
            Some(TextCommand::Resume) => {
                hexagon_core::orchestra::resume(&db, hexagon_core::PROJECT_ID)
                    .map_err(|e| e.to_string())?;
            }
            Some(other) => with_wb(&state, |wb| wb.dispatch_command(&other))?,
            None => {}
        }
        return Ok(id);
    }
    with_wb(&state, |wb| wb.send_message(&body))
}

#[tauri::command]
fn answer_permission(
    state: tauri::State<AppState>,
    question_id: String,
    allow: bool,
    remember_shape: Option<String>,
    scope: String,
) -> Result<(), String> {
    with_wb(&state, |wb| {
        wb.answer_permission(&question_id, allow, remember_shape.as_deref(), &scope)
    })
}

#[tauri::command]
fn advance(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.advance())
}
#[tauri::command]
fn run_checks(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.run_checks())
}
#[tauri::command]
fn stamp(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.stamp())
}
#[tauri::command]
fn rewind(state: tauri::State<AppState>, to_seq: usize) -> Result<Value, String> {
    with_wb(&state, |wb| wb.rewind(to_seq))
}
#[tauri::command]
fn skip(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.skip())
}
#[tauri::command]
fn skip_review(state: tauri::State<AppState>, artifact_kind: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.skip_review(&artifact_kind))
}
#[tauri::command]
fn pause(state: tauri::State<AppState>) -> Result<(), String> {
    // 票 04：暂停按钮是回合中的叫停通道——必须走旁路，wb 锁正被回合占着
    if let Some(db) = side_db(&state)? {
        return hexagon_core::orchestra::pause(&db, hexagon_core::PROJECT_ID)
            .map_err(|e| e.to_string());
    }
    with_wb(&state, |wb| wb.pause())
}
#[tauri::command]
fn resume(state: tauri::State<AppState>) -> Result<(), String> {
    if let Some(db) = side_db(&state)? {
        return hexagon_core::orchestra::resume(&db, hexagon_core::PROJECT_ID)
            .map_err(|e| e.to_string());
    }
    with_wb(&state, |wb| wb.resume())
}
#[tauri::command]
fn sleep_all(state: tauri::State<AppState>) -> Result<(), String> {
    with_wb(&state, |wb| wb.sleep_all())
}

#[tauri::command]
fn artifacts(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.artifacts())
}
#[tauri::command]
fn artifact_content(state: tauri::State<AppState>, path: String) -> Result<String, String> {
    with_wb(&state, |wb| wb.artifact_content(&path))
}
#[tauri::command]
fn team(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.team())
}
#[tauri::command]
fn stage_status(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.stage_status())
}
#[tauri::command]
fn pending_questions(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.pending_questions())
}
#[tauri::command]
fn usage(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.usage())
}

#[tauri::command]
fn open_stage(state: tauri::State<AppState>, seq: usize) -> Result<Value, String> {
    with_wb(&state, |wb| wb.open_stage(seq))
}

#[tauri::command]
fn recover_run(state: tauri::State<AppState>, run_id: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.recover_run(&run_id))
}

#[tauri::command]
fn override_checks(state: tauri::State<AppState>, reason: String) -> Result<Value, String> {
    with_wb(&state, |wb| wb.override_checks(&reason))
}

#[tauri::command]
fn request_install(state: tauri::State<AppState>, desc: String) -> Result<String, String> {
    with_wb(&state, |wb| wb.request_install(&desc))
}

#[tauri::command]
fn agent_detail(state: tauri::State<AppState>, agent_id: String) -> Result<Value, String> {
    with_wb(&state, |wb| wb.agent_detail(&agent_id))
}

#[tauri::command]
fn update_agent(
    state: tauri::State<AppState>,
    agent_id: String,
    patch: Value,
) -> Result<(), String> {
    let patch: hexagon_core::roles::AgentPatch =
        serde_json::from_value(patch).map_err(|e| e.to_string())?;
    with_wb(&state, |wb| wb.update_agent(&agent_id, patch))
}

#[tauri::command]
fn create_role(state: tauri::State<AppState>, def: Value) -> Result<String, String> {
    let def: hexagon_core::presets::RoleDef =
        serde_json::from_value(def).map_err(|e| e.to_string())?;
    with_wb(&state, |wb| wb.create_role(def))
}

#[tauri::command]
fn set_agent_grants(
    state: tauri::State<AppState>,
    agent_id: String,
    kind: String,
    names: Vec<String>,
) -> Result<(), String> {
    with_wb(&state, |wb| wb.set_agent_grants(&agent_id, &kind, names))
}

#[tauri::command]
fn draft_role_def(
    state: tauri::State<AppState>,
    agent_id: String,
    hint: String,
) -> Result<String, String> {
    with_wb(&state, |wb| wb.draft_role_def(&agent_id, &hint))
}

#[tauri::command]
fn pack_draft(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.pack_draft())
}

#[tauri::command]
fn save_pack_draft(state: tauri::State<AppState>, pack_json: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.save_pack_draft(&pack_json))
}

#[tauri::command]
fn save_pack_template(state: tauri::State<AppState>, pack_json: String) -> Result<String, String> {
    with_wb(&state, |wb| wb.save_pack_template(&pack_json))
}

#[tauri::command]
fn pack_templates(state: tauri::State<AppState>) -> Result<Vec<String>, String> {
    with_wb(&state, |wb| wb.pack_templates())
}

#[tauri::command]
fn export_pack_yaml(state: tauri::State<AppState>, dest: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.export_pack_yaml(&dest))
}

#[tauri::command]
fn resolve_install(
    state: tauri::State<AppState>,
    qid: String,
    allow: bool,
) -> Result<Value, String> {
    with_wb(&state, |wb| wb.resolve_install(&qid, allow))
}

#[tauri::command]
fn export_events(
    state: tauri::State<AppState>,
    path: String,
    stage_run_id: Option<String>,
    agent_id: Option<String>,
    kinds: Option<Vec<String>>,
) -> Result<usize, String> {
    let kinds = kinds
        .map(|ks| {
            ks.iter()
                .map(|k| {
                    serde_json::from_value::<hexagon_core::trace::EventKind>(serde_json::json!(k))
                        .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;
    with_wb(&state, |wb| {
        wb.export_events(
            std::path::Path::new(&path),
            &hexagon_core::trace::ExportFilter {
                stage_run_id,
                agent_id,
                kinds,
            },
        )
    })
}

#[tauri::command]
fn autonomy(state: tauri::State<AppState>) -> Result<String, String> {
    with_wb(&state, |wb| wb.autonomy())
}

#[tauri::command]
fn set_autonomy(state: tauri::State<AppState>, level: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.set_autonomy(&level))
}

#[tauri::command]
fn owner_away(state: tauri::State<AppState>) -> Result<(), String> {
    with_wb(&state, |wb| wb.owner_away())
}

#[tauri::command]
fn owner_back(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.owner_back())
}

#[tauri::command]
fn reject_stamp(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.reject_stamp())
}

#[tauri::command]
fn adjudicate_flag(
    state: tauri::State<AppState>,
    qid: String,
    agree: bool,
) -> Result<Value, String> {
    with_wb(&state, |wb| wb.adjudicate_flag(&qid, agree))
}

#[tauri::command]
fn proposals(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.proposals())
}

#[tauri::command]
fn review_proposal(
    state: tauri::State<AppState>,
    proposal_id: String,
    pass: bool,
    reason: String,
    reviewer_agent: String,
) -> Result<(), String> {
    with_wb(&state, |wb| {
        wb.review_proposal(&proposal_id, pass, &reason, &reviewer_agent)
    })
}

#[tauri::command]
fn confirm_proposal(state: tauri::State<AppState>, qid: String) -> Result<String, String> {
    with_wb(&state, |wb| wb.confirm_proposal(&qid))
}

#[tauri::command]
fn reject_proposal(
    state: tauri::State<AppState>,
    qid: String,
    reason: String,
) -> Result<(), String> {
    with_wb(&state, |wb| wb.reject_proposal(&qid, &reason))
}

/// 票 07：不变量伴随件手动体检入口——返回违规数（0=trace 自洽），
/// 违规明细落 invariant_violation 事件。
#[tauri::command]
fn invariant_check(state: tauri::State<AppState>) -> Result<usize, String> {
    with_wb(&state, |wb| wb.invariant_check_and_log())
}

/// 票 10：政策研发提案的确定性入口——旋钮编辑 JSON + 场景 JSON →
/// 副本改旋钮 → 回放 → 携证据+judge 判定进普通提案队列。
/// 产出物无特权通道（submit 全校验+负责人盖章不变）。
#[tauri::command]
fn policydev_propose(
    state: tauri::State<AppState>,
    edits: Vec<Value>,
    scenario: Value,
    motive: String,
) -> Result<String, String> {
    let sc: hexagon_core::scenario::Scenario =
        serde_json::from_value(scenario).map_err(|e| e.to_string())?;
    with_wb(&state, |wb| wb.policydev_propose(&edits, &sc, &motive))
}

#[tauri::command]
fn rollback_proposal(state: tauri::State<AppState>, proposal_id: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.rollback_proposal(&proposal_id))
}

#[tauri::command]
fn request_publish(state: tauri::State<AppState>, remote: String) -> Result<String, String> {
    with_wb(&state, |wb| wb.request_publish(&remote))
}

#[tauri::command]
fn confirm_publish(state: tauri::State<AppState>, qid: String) -> Result<Value, String> {
    with_wb(&state, |wb| wb.confirm_publish(&qid))
}

#[tauri::command]
fn reject_publish(state: tauri::State<AppState>, qid: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.reject_publish(&qid))
}

#[tauri::command]
fn set_agent_avatar(
    state: tauri::State<AppState>,
    agent_id: String,
    data_url: String,
) -> Result<(), String> {
    with_wb(&state, |wb| wb.set_agent_avatar(&agent_id, &data_url))
}

#[tauri::command]
fn agent_avatar(state: tauri::State<AppState>, agent_id: String) -> Result<Option<String>, String> {
    with_wb(&state, |wb| wb.agent_avatar(&agent_id))
}

#[tauri::command]
fn artifact_content_at(
    state: tauri::State<AppState>,
    path: String,
    version: i64,
) -> Result<Option<String>, String> {
    with_wb(&state, |wb| wb.artifact_content_at(&path, version))
}

#[tauri::command]
fn set_agent_sleeping(
    state: tauri::State<AppState>,
    agent_id: String,
    sleeping: bool,
) -> Result<(), String> {
    with_wb(&state, |wb| wb.set_agent_sleeping(&agent_id, sleeping))
}

#[tauri::command]
fn set_usage_limit(state: tauri::State<AppState>, limit_cents: Option<i64>) -> Result<(), String> {
    with_wb(&state, |wb| wb.set_usage_limit(limit_cents))
}

#[tauri::command]
fn usage_series(
    state: tauri::State<AppState>,
    granularity: String,
    from: Option<String>,
    to: Option<String>,
) -> Result<Value, String> {
    with_wb(&state, |wb| {
        wb.usage_series(&granularity, from.as_deref(), to.as_deref())
            .map(|v| serde_json::to_value(v).unwrap())
    })
}

/// 日志开关：app 级设置持久化在 config 目录，开发期默认开（Debug），release 默认 Info。
fn log_enabled_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

// ---------- 启动页 / 最近项目（票 29）：app 级 JSON，不进项目库 ----------

fn recents_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("recent_projects.json"))
}

fn read_recents(app: &tauri::AppHandle) -> Vec<Value> {
    recents_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default()
}

fn remember_recent(app: &tauri::AppHandle, dir: &str, name: &str, mode: &str) {
    let mut rs = read_recents(app);
    rs.retain(|r| r["dir"] != dir);
    rs.insert(
        0,
        serde_json::json!({
            "dir": dir, "name": name, "mode": mode,
            "opened_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }),
    );
    rs.truncate(10);
    if let Some(p) = recents_path(app) {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&p, serde_json::to_string(&rs).unwrap_or_default());
    }
}

#[tauri::command]
fn recent_projects(app: tauri::AppHandle) -> Vec<Value> {
    read_recents(&app)
        .into_iter()
        .filter(|r| r["dir"].as_str().map(|d| std::path::Path::new(d).is_dir()) == Some(true))
        .collect()
}

/// 重新打开已有项目：钉住的包副本恢复 pack，fastpath 项目无副本即 None。
#[tauri::command]
fn open_recent(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    dir: String,
) -> Result<(), String> {
    let d = std::path::Path::new(&dir);
    if !d.join(".hexagon/state.db").exists() {
        return Err(format!("不是 Hexagon 项目目录: {dir}"));
    }
    let pack = hexagon_core::orchestra::PackDef::pinned(d).ok();
    let name = d
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.clone());
    // OR IGNORE 保住库里的真实 name/mode；project_info 读库得真值
    let mut wb = Workbench::open(&dir, &name, &[], pack).map_err(|e| e.to_string())?;
    hexagon_core::providers::register_all(
        &mut wb.providers,
        Arc::new(hexagon_core::credentials::OsKeychain),
    );
    attach_delta_hook(&app, &wb);
    *state.db_path.lock().map_err(|e| e.to_string())? =
        Some(std::path::Path::new(&dir).join(".hexagon/state.db"));
    let info = wb.project_info().map_err(|e| e.to_string())?;
    *state.wb.lock().map_err(|e| e.to_string())? = Some(wb);
    remember_recent(
        &app,
        &dir,
        info["name"].as_str().unwrap_or(&name),
        info["mode"].as_str().unwrap_or("pack"),
    );
    Ok(())
}

/// 关闭当前项目回启动页（不删任何数据）。
#[tauri::command]
fn close_project(state: tauri::State<AppState>) -> Result<(), String> {
    *state.wb.lock().map_err(|e| e.to_string())? = None;
    *state.db_path.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

fn load_log_enabled(app: &tauri::AppHandle) -> bool {
    log_enabled_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["log_enabled"].as_bool())
        .unwrap_or(cfg!(debug_assertions)) // dev 默认开，release 默认关
}

#[tauri::command]
fn set_log_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let level = if enabled {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Warn
    };
    log::set_max_level(level);
    let p = log_enabled_path(&app).ok_or("no config dir")?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, serde_json::json!({"log_enabled": enabled}).to_string())
        .map_err(|e| e.to_string())?;
    log::info!("log level set to {level}");
    Ok(())
}

#[tauri::command]
fn log_enabled(app: tauri::AppHandle) -> bool {
    load_log_enabled(&app)
}

// ---------- 项目向导（票 24）：开项目前的检查/选择/建项目，不需要 wb ----------

#[tauri::command]
fn inspect_dir(dir: String) -> Value {
    serde_json::to_value(hexagon_core::setup::inspect_dir(&dir)).unwrap()
}

#[tauri::command]
fn preset_roles() -> Result<Value, String> {
    hexagon_core::presets::preset_roles()
        .map(|v| serde_json::to_value(v).unwrap())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn preset_packs() -> Result<Value, String> {
    hexagon_core::presets::preset_packs()
        .map(|v| serde_json::to_value(v).unwrap())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn check_model_keys(slots: Vec<String>) -> Result<Vec<String>, String> {
    use hexagon_core::credentials::{model_key_name, CredentialStore, OsKeychain};
    let mut missing = Vec::new();
    for s in slots {
        let name = model_key_name(&s);
        if OsKeychain.get(&name).map_err(|e| e.to_string())?.is_none() {
            missing.push(name);
        }
    }
    Ok(missing)
}

#[tauri::command]
fn set_model_key(slot: String, secret: String) -> Result<(), String> {
    use hexagon_core::credentials::{model_key_name, CredentialStore, OsKeychain};
    OsKeychain
        .set(&model_key_name(&slot), &secret)
        .map_err(|e| e.to_string())
}

// ---------- 供应商配置（设置页模型区 / 启动页共用）----------

/// 供应商文档：非密字段 + 各供应商 key 是否已存（key 明文永不回传）+ 槽位绑定表。
#[tauri::command]
fn list_providers() -> Result<Value, String> {
    use hexagon_core::credentials::{provider_key_name, CredentialStore, OsKeychain};
    let doc = hexagon_core::providers::load().map_err(|e| e.to_string())?;
    let providers: Vec<Value> = doc
        .providers
        .iter()
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap();
            v["key_set"] = serde_json::json!(OsKeychain
                .get(&provider_key_name(&p.id))
                .ok()
                .flatten()
                .is_some());
            v
        })
        .collect();
    Ok(serde_json::json!({
        "providers": providers,
        "slots": doc.slots,
    }))
}

/// 刷新运行中 Workbench 的供应商注册（保存/删除/绑定变更后热生效）。
fn refresh_providers(state: &AppState) {
    let mut g = match state.wb.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if let Some(wb) = g.as_mut() {
        // 清掉 HTTP 供应商注册再按当前绑定重挂（ScriptedProvider 是测试件，生产不会有）
        let doc = hexagon_core::providers::load().unwrap_or_default();
        for slot in doc.slots.keys() {
            wb.providers.remove(slot);
        }
        hexagon_core::providers::register_all(
            &mut wb.providers,
            Arc::new(hexagon_core::credentials::OsKeychain),
        );
    }
}

/// 保存供应商 + 可选 key；开着的项目热刷新。
#[tauri::command]
fn save_provider(
    state: tauri::State<AppState>,
    provider: Value,
    secret: Option<String>,
) -> Result<(), String> {
    use hexagon_core::credentials::{provider_key_name, CredentialStore, OsKeychain};
    let def: hexagon_core::providers::ProviderDef =
        serde_json::from_value(provider).map_err(|e| e.to_string())?;
    hexagon_core::providers::save_provider(&def).map_err(|e| e.to_string())?;
    if let Some(s) = secret.filter(|s| !s.trim().is_empty()) {
        OsKeychain
            .set(&provider_key_name(&def.id), s.trim())
            .map_err(|e| e.to_string())?;
    }
    refresh_providers(&state);
    Ok(())
}

/// 删供应商（级联解绑槽位）+ 热刷新；keychain 不动。
#[tauri::command]
fn delete_provider(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    hexagon_core::providers::delete_provider(&id).map_err(|e| e.to_string())?;
    refresh_providers(&state);
    Ok(())
}

/// 槽位绑定（供应商必须存在）+ 热刷新。
#[tauri::command]
fn set_slot_binding(
    state: tauri::State<AppState>,
    slot: String,
    provider_id: String,
    model: String,
) -> Result<(), String> {
    hexagon_core::providers::set_binding(&slot, &provider_id, &model).map_err(|e| e.to_string())?;
    refresh_providers(&state);
    Ok(())
}

/// 解绑槽位 + 热刷新。
#[tauri::command]
fn remove_slot_binding(state: tauri::State<AppState>, slot: String) -> Result<(), String> {
    hexagon_core::providers::remove_binding(&slot).map_err(|e| e.to_string())?;
    refresh_providers(&state);
    Ok(())
}

/// 拉取/检测供应商模型目录：GET /models；key 从 keychain 现取，缺 key 直报。
/// 返回 ModelEntry 表（id + 分组 + 推断能力），UI 合并进供应商配置。
#[tauri::command]
fn fetch_provider_models(id: String) -> Result<Value, String> {
    use hexagon_core::credentials::{provider_key_name, CredentialStore, OsKeychain};
    let doc = hexagon_core::providers::load().map_err(|e| e.to_string())?;
    let def = doc
        .providers
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("未知供应商: {id}"))?;
    let key = OsKeychain
        .get(&provider_key_name(&def.id))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("缺 API key: provider/{}", def.id))?;
    hexagon_core::providers::fetch_models(def, &key)
        .map(|m| serde_json::to_value(m).unwrap())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn agents_md_draft(name: String) -> String {
    hexagon_core::setup::agents_md_draft(&name)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProjectOpts {
    dir: String,
    name: String,
    roles: Vec<String>,
    pack_name: Option<String>,
    fastpath_role: Option<String>,
    init_git: bool,
    agents_md: Option<String>,
}

#[tauri::command]
fn create_project(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    opts: CreateProjectOpts,
) -> Result<(), String> {
    use hexagon_core::credentials::OsKeychain;
    use hexagon_core::setup;
    // 负责人已确认的说明文件先落盘（已存在会被 write_agents_md 拒绝，不覆盖）
    if let Some(md) = &opts.agents_md {
        setup::write_agents_md(&opts.dir, md).map_err(|e| e.to_string())?;
    }
    let pack = opts
        .pack_name
        .as_deref()
        .map(|n| {
            hexagon_core::presets::preset_packs()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|p| p.name == n)
                .ok_or_else(|| format!("未知流程包: {n}"))
        })
        .transpose()?;
    let pdoc = hexagon_core::providers::load().unwrap_or_default();
    let mut wb = setup::create_project(
        &opts.dir,
        &opts.name,
        &opts.roles,
        pack.as_ref(),
        opts.fastpath_role.as_deref(),
        opts.init_git,
        &OsKeychain,
        &pdoc,
    )
    .map_err(|e| e.to_string())?;
    hexagon_core::providers::register_all(&mut wb.providers, Arc::new(OsKeychain));
    attach_delta_hook(&app, &wb);
    *state.db_path.lock().map_err(|e| e.to_string())? =
        Some(std::path::Path::new(&opts.dir).join(".hexagon/state.db"));
    *state.wb.lock().map_err(|e| e.to_string())? = Some(wb);
    remember_recent(
        &app,
        &opts.dir,
        &opts.name,
        if opts.pack_name.is_some() {
            "pack"
        } else {
            "fastpath"
        },
    );
    Ok(())
}

#[tauri::command]
fn project_open(state: tauri::State<AppState>) -> bool {
    state.wb.lock().map(|g| g.is_some()).unwrap_or(false)
}

// ---------- 快速通道（票 26）----------

#[tauri::command]
fn project_info(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.project_info())
}

#[tauri::command]
fn dispatch(state: tauri::State<AppState>, role: String, input: String) -> Result<Value, String> {
    with_wb(&state, |wb| {
        wb.dispatch(&role, &input)
            .map(|o| serde_json::to_value(o).unwrap())
    })
}

#[tauri::command]
fn upgrade_to_pack(state: tauri::State<AppState>, pack_name: String) -> Result<(), String> {
    let pack = hexagon_core::presets::preset_packs()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.name == pack_name)
        .ok_or_else(|| format!("未知流程包: {pack_name}"))?;
    with_wb_mut(&state, |wb| wb.upgrade_to_pack(pack))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("hexagon".into()),
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                ])
                .max_file_size(5_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                .level(log::LevelFilter::Debug)
                .build(),
        )
        .setup(|app| {
            // 启动时按持久化设置收口级别（plugin 初始给 Debug 以便捕获启动日志）
            let enabled = load_log_enabled(app.handle());
            log::set_max_level(if enabled {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Warn
            });
            log::info!("hexagon-bot starting, log_enabled={enabled}");
            Ok(())
        })
        .manage(AppState {
            wb: Mutex::new(None),
            db_path: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            core_ping,
            open_project,
            timeline,
            send_message,
            answer_permission,
            advance,
            run_checks,
            stamp,
            rewind,
            skip,
            skip_review,
            pause,
            resume,
            sleep_all,
            artifacts,
            artifact_content,
            team,
            stage_status,
            pending_questions,
            usage,
            open_stage,
            recover_run,
            override_checks,
            agent_detail,
            update_agent,
            create_role,
            set_agent_grants,
            draft_role_def,
            pack_draft,
            save_pack_draft,
            save_pack_template,
            pack_templates,
            export_pack_yaml,
            request_install,
            resolve_install,
            export_events,
            autonomy,
            set_autonomy,
            owner_away,
            owner_back,
            reject_stamp,
            adjudicate_flag,
            proposals,
            review_proposal,
            confirm_proposal,
            reject_proposal,
            invariant_check,
            policydev_propose,
            rollback_proposal,
            request_publish,
            confirm_publish,
            reject_publish,
            set_agent_avatar,
            agent_avatar,
            artifact_content_at,
            set_agent_sleeping,
            set_usage_limit,
            usage_series,
            set_log_enabled,
            log_enabled,
            inspect_dir,
            preset_roles,
            preset_packs,
            check_model_keys,
            set_model_key,
            list_providers,
            save_provider,
            delete_provider,
            set_slot_binding,
            remove_slot_binding,
            fetch_provider_models,
            agents_md_draft,
            create_project,
            project_open,
            project_info,
            dispatch,
            upgrade_to_pack,
            recent_projects,
            open_recent,
            close_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

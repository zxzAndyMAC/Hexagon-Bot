//! 画师出图（owner-followups-1015 票 20 / 28）：出图槽已绑且支持时，项目经理召唤画师 → 画师的
//! 主对话模型配内置「出图提示词」技能为每个角色写提示词（假聊天模型）→ 逐角色调 OpenAI 兼容
//! `/images/generations`（本地一次性服务模拟，绝不打真接口）→ 头像提案。
//! 断言面 = 外发载荷（服务端收到的请求体 / 假聊天模型收到的请求）、轨迹事实、用量读面、
//! 提案读面、头像读面、待决卡。
use super::*;
use crate::provider::ScriptedProvider;
use crate::provider_config::{ProviderDef, ProviderDoc, SlotBinding};
use crate::turn::{text_response, tool_response};
use base64::Engine;
use serde_json::Value;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

const KEY: &str = "synthetic-image-key";
const OWNER_SAYS: &str = "给我的团队每个角色设置一个像素风格的头像，参考 src/secret_plan.md";

#[derive(Clone)]
enum Reply {
    Png,
    Status(u16),
}

struct Seen {
    path: String,
    auth: String,
    body: Value,
}

/// 一次性本地服务：按脚本逐个回应，记下每个请求的路径、鉴权头、JSON 体。
fn serve(script: Vec<Reply>) -> (String, Arc<Mutex<Vec<Seen>>>, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let handle = std::thread::spawn(move || {
        for reply in script {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let (head, body) = read_request(&mut socket);
            let mut lines = head.lines();
            let path = lines.next().unwrap().split(' ').nth(1).unwrap().to_string();
            let auth = head
                .lines()
                .find_map(|l| {
                    l.strip_prefix("authorization: ")
                        .or(l.strip_prefix("Authorization: "))
                })
                .unwrap_or("")
                .to_string();
            log.lock().unwrap().push(Seen {
                path,
                auth,
                body: serde_json::from_slice(&body).unwrap(),
            });
            let (status, text) = match reply {
                Reply::Png => (
                    200,
                    json!({"created":1,"data":[{"b64_json":png_b64()}],
                    "usage":{"input_tokens":40,"output_tokens":1000,"total_tokens":1040}})
                    .to_string(),
                ),
                Reply::Status(s) => (s, json!({"error":{"message":"synthetic"}}).to_string()),
            };
            write!(socket, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).unwrap();
        }
    });
    (base, seen, handle)
}

fn read_request(socket: &mut std::net::TcpStream) -> (String, Vec<u8>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = socket.read(&mut chunk).unwrap();
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_string();
            let len: usize = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse().unwrap())
                })
                .unwrap_or(0);
            while buf.len() < end + 4 + len {
                let n = socket.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
            }
            return (head, buf[end + 4..end + 4 + len].to_vec());
        }
    }
}

fn png_b64() -> String {
    let img = image::DynamicImage::new_rgb8(64, 48);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    base64::engine::general_purpose::STANDARD.encode(out.into_inner())
}

fn image_doc(base: &str) -> ProviderDoc {
    let mut doc = ProviderDoc::default();
    doc.providers.push(ProviderDef {
        id: "img".into(),
        name: "Img".into(),
        kind: crate::provider::ProviderKind::OpenAi,
        base_url: base.into(),
        models: vec![],
        enabled: true,
    });
    doc.slots.insert(
        "image".into(),
        SlotBinding {
            provider_id: "img".into(),
            model: "gpt-image-1".into(),
        },
    );
    doc
}

fn bench(dir: &std::path::Path) -> Workbench {
    let wb = Workbench::for_test(dir, &["项目经理", "后端"], None).unwrap();
    wb.creds
        .set(&crate::credentials::provider_key_name("img"), KEY)
        .unwrap();
    wb
}

/// 假聊天模型给每个角色写的提示词：带标记，模板拼不出来。
fn chat_prompt(role: &str) -> String {
    format!("CHAT-WROTE {role}: pixel art bust, warm palette, plain backdrop")
}

/// 画师的主对话模型按「出图提示词」技能的约定回 JSON（包在代码围栏里也认）。
fn chat_reply(roles: &[&str]) -> String {
    let prompts: Vec<Value> = roles
        .iter()
        .map(|r| json!({"role": r, "prompt": chat_prompt(r)}))
        .collect();
    format!("```json\n{}\n```", json!({ "prompts": prompts }))
}

/// 召唤画师。项目经理走自己的 `pm` 槽；画师的主对话槽（`chat`）挂 `artist` 这个假模型
/// ——None = 不挂（`chat` 与 `default` 都没绑 → 画师没有可用的主对话模型）。
fn summon_with(wb: &mut Workbench, artist: Option<Arc<ScriptedProvider>>) {
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb.db
        .conn()
        .execute("UPDATE agents SET model_slot='pm' WHERE id='a0'", [])
        .unwrap();
    wb.register_provider(
        "pm",
        Arc::new(ScriptedProvider::new(vec![
            tool_response(vec![("s1", "summon_artist", json!({"style": "pixel art"}))]),
            text_response("ok"),
            text_response(crate::pm_route::HOLD_TOKEN),
        ])),
    );
    if let Some(artist) = artist {
        wb.register_provider("chat", artist);
    }
    wb.run_instance("a0", OWNER_SAYS).unwrap();
}

fn artist_chat(reply: &str) -> Arc<ScriptedProvider> {
    Arc::new(ScriptedProvider::new(vec![text_response(reply)]))
}

fn summon(wb: &mut Workbench) -> Arc<ScriptedProvider> {
    let artist = artist_chat(&chat_reply(&["项目经理", "后端"]));
    summon_with(wb, Some(artist.clone()));
    artist
}

fn texts(req: &crate::provider::ChatRequest, role: crate::provider::Role) -> String {
    req.messages
        .iter()
        .filter(|m| m.role == role)
        .flat_map(|m| &m.content)
        .filter_map(|b| match b {
            crate::provider::ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn system(wb: &Workbench, kind: &str) -> Vec<Value> {
    wb.db
        .timeline("p1", None, 500, None)
        .unwrap()
        .into_iter()
        .filter(|it| it.event.kind == EventKind::System && it.event.payload["kind"] == kind)
        .map(|it| it.event.payload)
        .collect()
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

#[test]
fn bound_image_slot_draws_each_role_and_posts_a_proposal_that_is_not_a_pending_card() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = bench(dir.path());
    let (base, seen, server) = serve(vec![Reply::Png, Reply::Png]);
    let _doc = crate::provider_config::fixture_document(image_doc(&base));
    let artist = summon(&mut wb);
    server.join().unwrap();

    // 票 28：提示词由画师的主对话模型写——全队一次请求，系统层是内置「出图提示词」技能，
    // 用户层只有风格 + 角色名 + 职责摘要（不含负责人原话、项目文件、对话）。
    let asked = artist.recorded();
    assert_eq!(asked.len(), 1, "全队一次请求，画风一致");
    assert!(asked[0].tools.is_empty(), "写提示词不给工具");
    let system_text = texts(&asked[0], crate::provider::Role::System);
    assert!(
        system_text.contains("image prompt"),
        "系统层是出图提示词技能: {system_text}"
    );
    let user_text = texts(&asked[0], crate::provider::Role::User);
    for want in ["pixel art", "项目经理", "后端"] {
        assert!(user_text.contains(want), "{want} 在输入里: {user_text}");
    }
    assert!(!user_text.contains("secret_plan"), "{user_text}");
    assert!(!user_text.contains(OWNER_SAYS), "{user_text}");

    // 外发载荷：只有 model/prompt/n/size；prompt 原样是聊天模型为该角色写的那条。
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "每个角色一次");
    let mut drawn: Vec<&str> = Vec::new();
    for req in seen.iter() {
        assert_eq!(req.path, "/v1/images/generations");
        assert_eq!(req.auth, format!("Bearer {KEY}"));
        assert_eq!(keys(&req.body), vec!["model", "n", "prompt", "size"]);
        assert_eq!(req.body["model"], "gpt-image-1");
        let prompt = req.body["prompt"].as_str().unwrap();
        let role = ["项目经理", "后端"]
            .into_iter()
            .find(|r| prompt == chat_prompt(r))
            .unwrap_or_else(|| panic!("提示词来自聊天模型输出: {prompt}"));
        drawn.push(role);
    }
    drawn.sort();
    assert_eq!(drawn, vec!["后端", "项目经理"]);

    // 轨迹：每次外发前一条，载荷字段 = 外发的三样。
    let requests = system(&wb, "artist_image_request");
    assert_eq!(requests.len(), 2);
    for (ev, req) in requests.iter().zip(seen.iter()) {
        assert_eq!(keys(&ev["outbound"]), vec!["duty_summary", "role", "style"]);
        assert_eq!(ev["outbound"]["style"], "pixel art");
        assert_eq!(ev["prompt"], req.body["prompt"], "轨迹记的就是发出去的");
        assert_eq!(ev["speaker"], crate::artist::ARTIST_ROLE);
    }

    let proposals = system(&wb, "artist_avatar_proposal");
    assert_eq!(proposals.len(), 1);
    let id = proposals[0]["proposal_id"].as_str().unwrap();
    let p = crate::artist_proposal::load(&wb.repo_root, id).unwrap();
    assert_eq!(p.items.len(), 2);
    assert!(
        p.items.iter().all(|i| i.prompt == chat_prompt(&i.role)),
        "卡上折叠的提示词就是聊天模型写的"
    );
    assert!(p.items.iter().all(|i| i
        .image
        .as_deref()
        .is_some_and(|u| u.starts_with("data:image/png;base64,"))));
    assert!(p.applied.is_empty());
    assert!(
        system(&wb, "artist_image_model_missing").is_empty(),
        "已绑不回缺位提示"
    );
    assert!(
        crate::cards::queued(&wb.db, "p1").unwrap().is_empty(),
        "提案不进待决"
    );
    // 召唤本身不改头像：只有负责人点「全部应用」才写。
    assert!(crate::avatars::role_avatar(&wb.repo_root, "后端")
        .unwrap()
        .is_none());

    // 用量记在画师名下（不在任何 Agent 名下）：出图两次 + 写提示词一次（主对话槽）。
    let rows = crate::usage::summarize(&wb.db, "p1").unwrap();
    let artist: Vec<_> = rows
        .iter()
        .filter(|r| r.agent_id.as_deref() == Some(crate::usage::ARTIST_ACTOR))
        .collect();
    assert_eq!(artist.len(), 2, "{rows:?}");
    let image = artist
        .iter()
        .find(|r| r.model.as_deref() == Some("image"))
        .unwrap();
    assert_eq!(image.calls, 2);
    assert_eq!(image.prompt_tokens, 80);
    assert_eq!(image.completion_tokens, 2000);
    let chat = artist
        .iter()
        .find(|r| r.model.as_deref() == Some("chat"))
        .unwrap_or_else(|| panic!("写提示词记在画师名下: {rows:?}"));
    assert_eq!(chat.calls, 1);
    let series = crate::usage::series(&wb.db, "p1", "day", None, None).unwrap();
    assert!(series
        .iter()
        .any(|b| b.agent_id.as_deref() == Some("artist") && b.completion_tokens == 2000));
}

#[test]
fn failed_draw_marks_that_cell_and_regenerate_replaces_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = bench(dir.path());
    let (base, _seen, server) = serve(vec![Reply::Png, Reply::Status(500)]);
    let _doc = crate::provider_config::fixture_document(image_doc(&base));
    let artist = summon(&mut wb);
    server.join().unwrap();
    let id = system(&wb, "artist_avatar_proposal")[0]["proposal_id"]
        .as_str()
        .unwrap()
        .to_string();
    let p = crate::artist_proposal::load(&wb.repo_root, &id).unwrap();
    assert!(p.items[0].image.is_some());
    assert_eq!(p.items[1].image, None);
    assert_eq!(p.items[1].failure.as_deref(), Some("provider_unavailable"));
    let (ok_role, failed_role) = (p.items[0].role.clone(), p.items[1].role.clone());

    // 失败格不能应用：整单拒，什么都不写。
    let err = crate::artist_proposal::apply(
        &wb.db,
        &wb.repo_root,
        "p1",
        &id,
        &[ok_role.clone(), failed_role.clone()],
    )
    .unwrap_err();
    assert!(matches!(
        err,
        crate::artist_proposal::ArtistError::ArtistRoleNotInProposal(_)
    ));
    assert!(crate::avatars::role_avatar(&wb.repo_root, &ok_role)
        .unwrap()
        .is_none());

    // 换一张：负责人路径（无 Agent），同样先写轨迹、记画师账。
    let (base2, seen2, server2) = serve(vec![Reply::Png]);
    let _doc2 = crate::provider_config::fixture_document(image_doc(&base2));
    let access = crate::image_gen::ImageAccess::new(wb.creds.clone());
    let cx = crate::artist_proposal::DrawCtx {
        db: &wb.db,
        root: &wb.repo_root,
        project_id: "p1",
        agent_id: None,
        stage_run_id: None,
    };
    let p = crate::artist_proposal::regenerate(&cx, &access, &id, &failed_role).unwrap();
    server2.join().unwrap();
    // 换一张 = 同一条提示词再出一张图，不再找聊天模型改写（改写会让同一格前后画风漂移）。
    assert_eq!(seen2.lock().unwrap().len(), 1);
    assert_eq!(
        seen2.lock().unwrap()[0].body["prompt"],
        json!(chat_prompt(&failed_role))
    );
    assert_eq!(artist.recorded().len(), 1, "换一张不再调聊天模型");
    assert!(p.items[1].image.is_some());
    assert_eq!(p.items[1].failure, None);
    assert_eq!(system(&wb, "artist_image_request").len(), 3);
}

#[test]
fn apply_writes_role_avatars_for_the_picked_cells_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = bench(dir.path());
    let (base, _seen, server) = serve(vec![Reply::Png, Reply::Png]);
    let _doc = crate::provider_config::fixture_document(image_doc(&base));
    summon(&mut wb);
    server.join().unwrap();
    let id = system(&wb, "artist_avatar_proposal")[0]["proposal_id"]
        .as_str()
        .unwrap()
        .to_string();
    // 「项目经理」那格被跳过：只应用「后端」。
    let p =
        crate::artist_proposal::apply(&wb.db, &wb.repo_root, "p1", &id, &["后端".into()]).unwrap();
    assert_eq!(p.applied, vec!["后端"]);
    assert!(crate::avatars::role_avatar(&wb.repo_root, "后端")
        .unwrap()
        .is_some());
    assert!(crate::avatars::role_avatar(&wb.repo_root, "项目经理")
        .unwrap()
        .is_none());
    // 票 17 的读取优先级照旧：后端实例 a1 读到的是角色头像。
    assert!(crate::avatars::agent_avatar(&wb.db, &wb.repo_root, "a1")
        .unwrap()
        .is_some());
    let applied = system(&wb, "artist_avatars_applied");
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0]["roles"], json!(["后端"]));
}

#[test]
fn proposal_ids_cannot_walk_out_of_the_artist_directory() {
    let dir = tempfile::tempdir().unwrap();
    for id in ["../avatars", "ap", "ap1/../../x", "x1"] {
        assert!(matches!(
            crate::artist_proposal::load(dir.path(), id),
            Err(crate::artist_proposal::ArtistError::ArtistProposalMissing(
                _
            ))
        ));
    }
}

#[test]
fn non_openai_image_provider_gets_the_unsupported_notice_without_any_request() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = bench(dir.path());
    let mut doc = image_doc("http://127.0.0.1:9/v1");
    doc.providers[0].kind = crate::provider::ProviderKind::Anthropic;
    let _doc = crate::provider_config::fixture_document(doc);
    summon(&mut wb);
    let notices = system(&wb, "artist_image_model_missing");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0]["state"], "unsupported");
    assert_eq!(notices[0]["reason"], "unsupported_provider");
    assert!(system(&wb, "artist_image_request").is_empty());
}

/// 不接任何连接的出图端点：用来证明一次出图请求都没发。
fn silent_endpoint() -> (std::net::TcpListener, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    (listener, base)
}

/// 主对话模型写坏了（不是 JSON / 漏了角色 / 空提示词）：画师明确说写不出来，一张图都不发，
/// 不静默退回固定模板（票 28 负责人裁决 2026-10-20）。
#[test]
fn unusable_chat_output_posts_a_prompt_failure_and_sends_no_image_request() {
    for reply in [
        "Sure! Here are some lovely avatars.".to_string(),
        chat_reply(&["后端"]),
        json!({"prompts":[{"role":"项目经理","prompt":"  "},{"role":"后端","prompt":"x"}]})
            .to_string(),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut wb = bench(dir.path());
        let (listener, base) = silent_endpoint();
        let _doc = crate::provider_config::fixture_document(image_doc(&base));
        let artist = artist_chat(&reply);
        summon_with(&mut wb, Some(artist.clone()));
        assert_eq!(artist.recorded().len(), 1);
        assert!(listener.accept().is_err(), "没有发任何出图请求: {reply}");
        let failed = system(&wb, "artist_prompt_failed");
        assert_eq!(failed.len(), 1, "{reply}");
        assert_eq!(failed[0]["reason"], "invalid_prompts");
        assert_eq!(failed[0]["speaker"], crate::artist::ARTIST_ROLE);
        assert!(system(&wb, "artist_image_request").is_empty());
        assert!(system(&wb, "artist_avatar_proposal").is_empty());
        assert!(crate::cards::queued(&wb.db, "p1").unwrap().is_empty());
    }
}

/// 画师的主对话槽（及 default 兜底）都没有模型：明确提示并指向主对话槽，不发出图请求。
#[test]
fn missing_chat_model_posts_a_prompt_failure_pointing_at_the_chat_slot() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = bench(dir.path());
    let (listener, base) = silent_endpoint();
    let _doc = crate::provider_config::fixture_document(image_doc(&base));
    summon_with(&mut wb, None);
    assert!(listener.accept().is_err(), "没有发任何出图请求");
    let failed = system(&wb, "artist_prompt_failed");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["reason"], "chat_model_missing");
    assert_eq!(failed[0]["slot"], "chat");
    assert!(system(&wb, "artist_avatar_proposal").is_empty());
}

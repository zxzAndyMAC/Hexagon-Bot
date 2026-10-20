//! 画师出图（owner-followups-1015 票 20）：出图槽已绑且支持时，项目经理召唤画师 → 逐角色
//! 调 OpenAI 兼容 `/images/generations`（本地一次性服务模拟，绝不打真接口）→ 头像提案。
//! 断言面 = 外发载荷（服务端收到的请求体）、轨迹事实、用量读面、提案读面、头像读面、待决卡。
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

fn summon(wb: &mut Workbench) {
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(vec![
            tool_response(vec![("s1", "summon_artist", json!({"style": "pixel art"}))]),
            text_response("ok"),
            text_response(crate::pm_route::HOLD_TOKEN),
        ])),
    );
    wb.run_instance("a0", OWNER_SAYS).unwrap();
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
    summon(&mut wb);
    server.join().unwrap();

    // 外发载荷：只有 model/prompt/n/size；prompt 只含风格 + 角色名 + 职责摘要。
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "每个角色一次");
    let mut drawn: Vec<&str> = Vec::new();
    for req in seen.iter() {
        assert_eq!(req.path, "/v1/images/generations");
        assert_eq!(req.auth, format!("Bearer {KEY}"));
        assert_eq!(keys(&req.body), vec!["model", "n", "prompt", "size"]);
        assert_eq!(req.body["model"], "gpt-image-1");
        let prompt = req.body["prompt"].as_str().unwrap();
        assert!(prompt.starts_with("pixel art."), "统一风格前缀: {prompt}");
        let role = ["项目经理", "后端"]
            .into_iter()
            .find(|r| prompt.contains(&format!("\"{r}\"")))
            .unwrap_or_else(|| panic!("提示词点名角色: {prompt}"));
        drawn.push(role);
        assert!(
            !prompt.contains("secret_plan"),
            "不含负责人原话 / 项目文件: {prompt}"
        );
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

    // 用量记在画师名下（不在任何 Agent 名下）。
    let rows = crate::usage::summarize(&wb.db, "p1").unwrap();
    let artist: Vec<_> = rows
        .iter()
        .filter(|r| r.agent_id.as_deref() == Some(crate::usage::ARTIST_ACTOR))
        .collect();
    assert_eq!(artist.len(), 1, "{rows:?}");
    assert_eq!(artist[0].calls, 2);
    assert_eq!(artist[0].prompt_tokens, 80);
    assert_eq!(artist[0].completion_tokens, 2000);
    assert_eq!(artist[0].model.as_deref(), Some("image"));
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
    summon(&mut wb);
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
    assert_eq!(seen2.lock().unwrap().len(), 1);
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

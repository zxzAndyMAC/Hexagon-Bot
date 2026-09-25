//! role_draft_live — 「AI 起草职责」的活测接缝（wizard 起草回路的真实验证面）。
//!
//! 走生产同款：providers.json（HEXAGON_PROVIDERS_PATH）→ role_draft 槽
//! 解析 → 真实供应商 → setup::draft_role_defs（含薄稿补跑）。凭据用 dev
//! 文件缝（HEXAGON_CREDENTIALS_PATH，与调试壳同一文件库）。零 mock。
//!
//! 用法：
//!   HEXAGON_CREDENTIALS_PATH=~/.config/hexagon/credentials.json \
//!     cargo run -p hexagon-core --example role_draft_live -- \
//!       --brief "俄罗斯方块移动端游戏" [--roles 产品策划,QA]

use hexagon_core::credentials::CredentialStore;
use hexagon_core::presets::{preset_roles, RoleDef};
use hexagon_core::provider_config;
use std::sync::Arc;

fn arg(flag: &str) -> Option<String> {
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == flag {
            return it.next();
        }
    }
    None
}

fn main() {
    let brief = arg("--brief").unwrap_or_else(|| {
        "俄罗斯方块移动端游戏：触屏操作的休闲手游，经典下落方块玩法，单机关卡 + 好友排行榜".into()
    });
    let picked: Vec<String> = arg("--roles")
        .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
        .unwrap_or_default();

    let doc = provider_config::load().expect("providers.json");
    let creds: Arc<dyn CredentialStore> = hexagon_core::credentials::active();

    let binding = provider_config::resolve_slot(&doc.slots, provider_config::ROLE_DRAFT_SLOT)
        .expect("role_draft slot (default fallback included)");
    let def = doc
        .providers
        .iter()
        .find(|p| p.id == binding.provider_id)
        .expect("role_draft provider exists");
    println!("槽位: role_draft → {}/{}", def.id, binding.model);
    println!(
        "凭据键 {}: {}",
        hexagon_core::credentials::provider_key_name(&def.id),
        match creds.get(&hexagon_core::credentials::provider_key_name(&def.id)) {
            Ok(Some(_)) => "present".into(),
            Ok(None) => "MISSING".into(),
            Err(e) => format!("ERR {e}"),
        }
    );
    println!(
        "模型输出上限: {:?}",
        provider_config::max_output_of(def, &binding.model)
    );

    let all = preset_roles().expect("preset roles");
    let roles: Vec<RoleDef> = if picked.is_empty() {
        all
    } else {
        all.into_iter()
            .filter(|r| picked.iter().any(|p| p == &r.name))
            .collect()
    };
    println!("入参角色 {} 个:", roles.len());
    for r in &roles {
        println!("  {} | 模板职责: {}", r.name, r.duty);
    }

    let started = std::time::Instant::now();
    let provider = provider_config::make_provider(def, &binding.model, creds);
    match hexagon_core::setup::draft_role_defs(&brief, &roles, provider.as_ref()) {
        Ok(seeds) => {
            println!(
                "\n=== {}ms，返回 {} 条 ===",
                started.elapsed().as_millis(),
                seeds.len()
            );
            for s in &seeds {
                let ends = s.duty.matches(['。', '！', '？', '!', '?', '.']).count();
                let thin = if ends < 2 { " ← 薄稿" } else { "" };
                println!(
                    "\n【{}】{}（{}字 {ends}句读{thin}）",
                    s.name,
                    s.duty,
                    s.duty.chars().count()
                );
            }
            let missing: Vec<_> = roles
                .iter()
                .filter(|r| !seeds.iter().any(|s| s.name == r.name))
                .map(|r| r.name.as_str())
                .collect();
            if !missing.is_empty() {
                println!("\n漏稿: {missing:?}");
            }
        }
        Err(e) => println!("\n=== {e} ==="),
    }
}

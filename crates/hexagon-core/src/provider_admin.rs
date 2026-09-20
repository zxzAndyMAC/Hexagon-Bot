//! 供应商配置面（arch-review 票 05 / ADR 0053）：providers 文档读写 +
//! keychain 凭据的组合操作，壳层唯一入口——`rg "hexagon_core::providers::"
//! src-tauri` 应保持 ≤1（唯一例外是 create_project 建档校验用的 load）。
//! 运行中实例的热挂接走 `Workbench::reload_providers`，不在此列。

use crate::credentials::{provider_key_name, CredentialStore};
use crate::providers::{self, ProviderDef};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error(transparent)]
    Providers(#[from] providers::ProvidersError),
    #[error(transparent)]
    Cred(#[from] crate::credentials::CredError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    #[error("missing API key: provider/{0}")]
    MissingKey(String),
}

/// 供应商文档的对外视图：非密字段 + 各供应商 key 是否已存
/// （key 明文永不回传）+ 槽位绑定表。
pub fn list(store: &dyn CredentialStore) -> Result<Value, AdminError> {
    let doc = providers::load()?;
    let providers: Vec<Value> = doc
        .providers
        .iter()
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or_default();
            v["key_set"] = serde_json::json!(store.get(&provider_key_name(&p.id))?.is_some());
            Ok(v)
        })
        .collect::<Result<_, AdminError>>()?;
    Ok(serde_json::json!({"providers": providers, "slots": doc.slots}))
}

/// 保存供应商 + 可选 key（空 key 不写）。热刷新由调用方（壳）另行触发。
pub fn save(
    def_json: Value,
    secret: Option<String>,
    store: &dyn CredentialStore,
) -> Result<(), AdminError> {
    let def: ProviderDef = serde_json::from_value(def_json)?;
    providers::save_provider(&def)?;
    if let Some(s) = secret.filter(|s| !s.trim().is_empty()) {
        store.set(&provider_key_name(&def.id), s.trim())?;
    }
    Ok(())
}

/// 删供应商（级联解绑槽位）；keychain 不动。
pub fn delete(id: &str) -> Result<(), AdminError> {
    Ok(providers::delete_provider(id)?)
}

/// 槽位绑定（供应商必须存在）。
pub fn set_binding(slot: &str, provider_id: &str, model: &str) -> Result<(), AdminError> {
    Ok(providers::set_binding(slot, provider_id, model)?)
}

/// 解绑槽位。
pub fn remove_binding(slot: &str) -> Result<(), AdminError> {
    Ok(providers::remove_binding(slot)?)
}

/// 拉取/检测供应商模型目录：GET /models；key 从凭据库现取，缺 key 直报。
pub fn fetch_models(id: &str, store: &dyn CredentialStore) -> Result<Value, AdminError> {
    let doc = providers::load()?;
    let def = doc
        .providers
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AdminError::UnknownProvider(id.into()))?;
    let key = store
        .get(&provider_key_name(&def.id))?
        .ok_or_else(|| AdminError::MissingKey(def.id.clone()))?;
    let models = providers::fetch_models(def, &key)?;
    Ok(serde_json::to_value(models)?)
}

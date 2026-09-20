//! 供应商配置面（arch-review 票 05 / ADR 0053）：providers 文档读写 +
//! keychain 凭据的组合操作，壳层唯一入口——`rg "hexagon_core::provider_config::"
//! src-tauri` 应保持 ≤1（唯一例外是 create_project 建档校验用的 load）。
//! 运行中实例的热挂接走 `Workbench::reload_providers`，不在此列。

use crate::credentials::{provider_key_name, CredentialStore};
use crate::provider_config;

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error(transparent)]
    Providers(#[from] provider_config::ProvidersError),
    #[error(transparent)]
    Cred(#[from] crate::credentials::CredError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    #[error("missing API key: provider/{0}")]
    MissingKey(String),
}

// 壳层供应商面的类型出口也在这里——provider_config:: 直名只许出现在本模块
// 与 create_project 建档校验（票 05 收口约定，rg 闸=1 针对逻辑直调）。
pub use crate::provider_config::{ModelEntry, ProviderDef};

/// 供应商的对外视图（ADR 0054）：非密字段平铺 + key_set 只报是否已存
/// （key 明文永不回传）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ProviderView {
    #[serde(flatten)]
    pub def: ProviderDef,
    pub key_set: bool,
}

/// providers.json 文档的对外视图：供应商表 + 槽位绑定表。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ProvidersView {
    pub providers: Vec<ProviderView>,
    pub slots: std::collections::HashMap<String, provider_config::SlotBinding>,
}

/// 供应商文档的对外视图：非密字段 + 各供应商 key 是否已存
/// （key 明文永不回传）+ 槽位绑定表。
pub fn list(store: &dyn CredentialStore) -> Result<ProvidersView, AdminError> {
    let doc = provider_config::load()?;
    let providers: Vec<ProviderView> = doc
        .providers
        .iter()
        .map(|p| {
            Ok(ProviderView {
                def: p.clone(),
                key_set: store.get(&provider_key_name(&p.id))?.is_some(),
            })
        })
        .collect::<Result<_, AdminError>>()?;
    Ok(ProvidersView {
        providers,
        slots: doc.slots,
    })
}

/// 保存供应商 + 可选 key（空 key 不写）。热刷新由调用方（壳）另行触发。
pub fn save(
    def: ProviderDef,
    secret: Option<String>,
    store: &dyn CredentialStore,
) -> Result<(), AdminError> {
    provider_config::save_provider(&def)?;
    if let Some(s) = secret.filter(|s| !s.trim().is_empty()) {
        store.set(&provider_key_name(&def.id), s.trim())?;
    }
    Ok(())
}

/// 删供应商（级联解绑槽位）；keychain 不动。
pub fn delete(id: &str) -> Result<(), AdminError> {
    Ok(provider_config::delete_provider(id)?)
}

/// 槽位绑定（供应商必须存在）。
pub fn set_binding(slot: &str, provider_id: &str, model: &str) -> Result<(), AdminError> {
    Ok(provider_config::set_binding(slot, provider_id, model)?)
}

/// 解绑槽位。
pub fn remove_binding(slot: &str) -> Result<(), AdminError> {
    Ok(provider_config::remove_binding(slot)?)
}

/// 拉取/检测供应商模型目录：GET /models；key 从凭据库现取，缺 key 直报。
pub fn fetch_models(
    id: &str,
    store: &dyn CredentialStore,
) -> Result<Vec<provider_config::ModelEntry>, AdminError> {
    let doc = provider_config::load()?;
    let def = doc
        .providers
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AdminError::UnknownProvider(id.into()))?;
    let key = store
        .get(&provider_key_name(&def.id))?
        .ok_or_else(|| AdminError::MissingKey(def.id.clone()))?;
    let models = provider_config::fetch_models(def, &key)?;
    Ok(models)
}

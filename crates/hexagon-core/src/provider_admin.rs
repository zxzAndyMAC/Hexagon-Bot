//! 供应商配置面（arch-review 票 05 / ADR 0053）：providers 文档读写 +
//! keychain 凭据的组合操作，壳层唯一入口——`rg "hexagon_core::provider_config::"
//! src-tauri` 应保持 ≤1（唯一例外是 create_project 建档校验用的 load）。
//! 运行中实例的热挂接走 `Workbench::reload_providers`，不在此列。

use std::sync::Arc;

use crate::credentials::{provider_key_name, CredentialStore};
use crate::provider::ModelProvider;
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
    /// 票 16：优化描述要的是主对话模型（default 槽），不是任意一个已配槽。
    #[error("主对话模型未就绪：需要启用的供应商、钥匙和 default 槽")]
    MainChatUnavailable,
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

/// 票 16：向导优化项目说明用的主对话模型。项目还不存在，不走 Workbench。
/// 主对话 = default 槽（票 13 向导第一步放行的那一条），不是角色槽。
/// 缺绑定、供应商不存在、停用、空模型名、没钥匙 → 不发请求。
/// 起草槽。`resolve_slot` 在槽未绑时落到 default。Jev 不走这里。
pub fn authoring_provider(
    store: Arc<dyn CredentialStore>,
    slot: &str,
) -> Result<Arc<dyn ModelProvider>, AdminError> {
    let doc = provider_config::load()?;
    let Some(binding) = provider_config::resolve_slot(&doc.slots, slot) else {
        return Err(AdminError::MainChatUnavailable);
    };
    let Some(def) = doc.providers.iter().find(|p| p.id == binding.provider_id) else {
        return Err(AdminError::MainChatUnavailable);
    };
    if !def.enabled || binding.model.trim().is_empty() {
        return Err(AdminError::MainChatUnavailable);
    }
    let key = store.get(&provider_key_name(&def.id))?;
    if key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return Err(AdminError::MainChatUnavailable);
    }
    Ok(provider_config::make_provider(def, binding.model.as_str(), store))
}

pub fn main_chat_provider(
    store: Arc<dyn CredentialStore>,
) -> Result<Arc<dyn ModelProvider>, AdminError> {
    let doc = provider_config::load()?;
    let (def, model) = require_main_chat(&doc, store.as_ref())?;
    Ok(provider_config::make_provider(def, model, store))
}

pub(crate) fn require_main_chat<'a>(
    doc: &'a provider_config::ProviderDoc,
    store: &dyn CredentialStore,
) -> Result<(&'a provider_config::ProviderDef, &'a str), AdminError> {
    // resolve_slot 是回退链的唯一实现。这里问的就是 default 本身。
    let Some(binding) = provider_config::resolve_slot(&doc.slots, "default") else {
        return Err(AdminError::MainChatUnavailable);
    };
    let Some(def) = doc.providers.iter().find(|p| p.id == binding.provider_id) else {
        return Err(AdminError::MainChatUnavailable);
    };
    if !def.enabled || binding.model.trim().is_empty() {
        return Err(AdminError::MainChatUnavailable);
    }
    let key = store.get(&provider_key_name(&def.id))?;
    if key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return Err(AdminError::MainChatUnavailable);
    }
    Ok((def, binding.model.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryStore;
    use crate::provider::ProviderKind;
    use crate::provider_config::{ProviderDef, ProviderDoc, SlotBinding};

    fn doc_default(enabled: bool) -> ProviderDoc {
        let mut doc = ProviderDoc::default();
        doc.providers.push(ProviderDef {
            id: "p".into(),
            name: "P".into(),
            kind: ProviderKind::OpenAi,
            base_url: "http://localhost".into(),
            models: vec![],
            enabled,
        });
        doc.slots.insert(
            "default".into(),
            SlotBinding {
                provider_id: "p".into(),
                model: "m".into(),
            },
        );
        doc
    }

    #[test]
    fn main_chat_is_the_default_slot_only() {
        let store = MemoryStore::default();
        store.set("provider/p", "sk").unwrap();
        let ready = doc_default(true);
        let (def, model) = require_main_chat(&ready, &store).unwrap();
        assert_eq!(def.id, "p");
        assert_eq!(model, "m");

        let mut chat_only = doc_default(true);
        chat_only.slots.clear();
        chat_only.slots.insert(
            "chat".into(),
            SlotBinding {
                provider_id: "p".into(),
                model: "m".into(),
            },
        );
        assert!(matches!(
            require_main_chat(&chat_only, &store),
            Err(AdminError::MainChatUnavailable)
        ));
        assert!(matches!(
            require_main_chat(&doc_default(false), &store),
            Err(AdminError::MainChatUnavailable)
        ));
        let bare = MemoryStore::default();
        assert!(matches!(
            require_main_chat(&doc_default(true), &bare),
            Err(AdminError::MainChatUnavailable)
        ));
    }
}

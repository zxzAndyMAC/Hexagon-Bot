//! Ticket 24: execution-side observations, never fields supplied by tool output.
//! Missing observations cost investigation; fabricated ones bless side effects.
use super::ToolContext;
use crate::artifacts::evidence::ArtifactEvidence;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum NativeEffect {
    // This is a bound on the native traversal, not a list inferred from hits.
    RepositorySearch,
    ShellCompleted {
        write_paths: Vec<String>,
        profile_digest: String,
        process_id: u32,
    },
    SkillLoad {
        name: String,
        source: String,
        source_digest: String,
    },
    FileRead {
        path: String,
        digest: String,
    },
    FileWrite {
        path: String,
        digest: String,
    },
    ArtifactRead {
        evidence: ArtifactEvidence,
    },
    ArtifactWrite {
        evidence: ArtifactEvidence,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EffectReceipt {
    pub version: u8,
    pub action_id: String,
    pub tool: String,
    pub root: String,
    pub input_digest: String,
    pub output_digest: String,
    pub effect: NativeEffect,
}

impl ToolContext {
    pub(crate) fn observe_native_effect(&self, effect: NativeEffect) {
        if let Some(slot) = &self.native_effect {
            *slot.lock().unwrap() = Some(effect);
        }
    }

    pub(crate) fn observe_file(&self, path: &Path, bytes: &[u8], write: bool) {
        use sha2::{Digest, Sha256};
        if self.native_effect.is_none() {
            return;
        }
        let Ok(root) = self.repo_root.canonicalize() else {
            return;
        };
        let Ok(path) = path.strip_prefix(root) else {
            return;
        };
        let path = path.to_string_lossy().into_owned();
        let digest = format!("{:x}", Sha256::digest(bytes));
        self.observe_native_effect(if write {
            NativeEffect::FileWrite { path, digest }
        } else {
            NativeEffect::FileRead { path, digest }
        });
    }
}

pub(crate) fn receipt(
    ctx: &ToolContext,
    id: &str,
    tool: &str,
    input: &Value,
    output: &Value,
) -> Option<EffectReceipt> {
    Some(EffectReceipt {
        version: 1,
        action_id: id.into(),
        tool: tool.into(),
        root: ctx
            .repo_root
            .canonicalize()
            .ok()?
            .to_string_lossy()
            .into_owned(),
        input_digest: crate::evaluation::config::digest(input).ok()?,
        output_digest: crate::evaluation::config::digest(output).ok()?,
        effect: ctx.native_effect.as_ref()?.lock().ok()?.clone()?,
    })
}

impl EffectReceipt {
    /// Test3/03: the caller supplies only receipts from the validated original
    /// parent chain. Missing/stale proof costs a reread; false acceptance loses edits.
    pub(crate) fn restore_edit_read(&self, ctx: &ToolContext) -> bool {
        use sha2::{Digest, Sha256};
        let (path, digest) = match &self.effect {
            NativeEffect::FileRead { path, digest } if self.tool == "fs_read" => (path, digest),
            NativeEffect::FileWrite { path, digest }
                if matches!(self.tool.as_str(), "fs_write" | "fs_patch") =>
            {
                (path, digest)
            }
            _ => return false,
        };
        let Ok(root) = ctx.repo_root.canonicalize() else {
            return false;
        };
        let Ok(target) = super::agent_readable_repo_path(&ctx.repo_root, path) else {
            return false;
        };
        // Recheck after collection: a later alias swap cannot redirect the proof.
        if Path::new(path).is_absolute() || target != root.join(path) {
            return false;
        }
        let path = target;
        let Some(before) = super::readstate::stamp(&path) else {
            return false;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        if format!("{:x}", Sha256::digest(&bytes)) != *digest
            || super::readstate::stamp(&path) != Some(before)
        {
            return false;
        }
        ctx.reads.record_stamp(&path, before);
        true
    }

    pub(crate) fn binds(
        &self,
        root: &Path,
        id: &str,
        tool: &str,
        input: &Value,
        output: &Value,
    ) -> bool {
        self.version == 1
            && self.action_id == id
            && self.tool == tool
            && root
                .canonicalize()
                .is_ok_and(|r| r.to_string_lossy() == self.root)
            && crate::evaluation::config::digest(input).is_ok_and(|d| d == self.input_digest)
            && crate::evaluation::config::digest(output).is_ok_and(|d| d == self.output_digest)
    }
}

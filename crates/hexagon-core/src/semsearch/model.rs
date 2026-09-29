//! A15: offline multilingual embeddings. Assets are installed explicitly;
//! inference has no downloader, model key, subprocess, or network client.
use super::{Checkpoint, Embedder};
use crate::tools::ToolError;
use fastembed::{
    InitOptionsUserDefined, OutputKey, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

pub(super) const NAME: &str = "embeddinggemma-q4-5090578-search-512-v1";

#[derive(Deserialize)]
struct Manifest {
    directory: String,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    sha256: String,
}

fn failure(error: impl std::fmt::Display) -> ToolError {
    ToolError::Exec(format!("local embedding model: {error}"))
}

// A15 review 2026-09-29: waiting on another project's model must remain
// cancellable. Native session initialization and one four-text batch finish
// before a stop can be observed; no unbounded file-sized inference call.
fn lock<'a, T>(mutex: &'a Mutex<T>, check: Checkpoint<'_>) -> Result<MutexGuard<'a, T>, ToolError> {
    loop {
        check()?;
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => return Err(failure("model unavailable")),
            Err(TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
        }
    }
}

pub(super) fn default_embedder(check: Checkpoint<'_>) -> Result<Arc<dyn Embedder>, ToolError> {
    static MODEL: Mutex<Option<Arc<dyn Embedder>>> = Mutex::new(None);
    let mut cache = lock(&MODEL, check)?;
    if let Some(model) = cache.as_ref() {
        return Ok(model.clone());
    }
    let manifest: Manifest = serde_json::from_str(include_str!("model.json")).map_err(failure)?;
    let explicit = std::env::var_os("HEXAGON_EMBEDDING_MODEL_DIR");
    let root = explicit.as_ref().map(PathBuf::from).or_else(|| {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| {
                PathBuf::from(home)
                    .join(".hexagon/models")
                    .join(manifest.directory)
            })
    });
    let Some(root) = root.filter(|root| explicit.is_some() || root.exists()) else {
        // Missing assets retain the existing zero-download text engine. The
        // tool exposes its engine, so this cannot masquerade as learned semantics.
        return Ok(Arc::new(super::HashEmbedder));
    };
    let model: Arc<dyn Embedder> = Arc::new(LocalEmbedder::open(&root, check)?);
    *cache = Some(model.clone());
    Ok(model)
}

pub struct LocalEmbedder(Mutex<TextEmbedding>);

impl LocalEmbedder {
    pub fn open(root: &Path, check: Checkpoint<'_>) -> Result<Self, ToolError> {
        let manifest: Manifest =
            serde_json::from_str(include_str!("model.json")).map_err(failure)?;
        let read = |name: &str| -> Result<Vec<u8>, ToolError> {
            check()?;
            let asset = manifest
                .assets
                .iter()
                .find(|asset| asset.name == name)
                .ok_or_else(|| failure("asset absent from manifest"))?;
            let mut bytes = Vec::new();
            std::fs::File::open(root.join(name))
                .map_err(failure)?
                .take(asset.size + 1)
                .read_to_end(&mut bytes)
                .map_err(failure)?;
            if bytes.len() as u64 != asset.size
                || format!("{:x}", Sha256::digest(&bytes)) != asset.sha256
            {
                return Err(failure(format!(
                    "{name} failed integrity check; reinstall the pinned model"
                )));
            }
            check()?;
            Ok(bytes)
        };
        // Pin tokenizer and graph together: accepting arbitrary files under a
        // fixed index signature would silently reuse incompatible old vectors.
        let mut model = UserDefinedEmbeddingModel::new(
            read("model_q4.onnx")?,
            TokenizerFiles {
                tokenizer_file: read("tokenizer.json")?,
                config_file: read("config.json")?,
                special_tokens_map_file: read("special_tokens_map.json")?,
                tokenizer_config_file: read("tokenizer_config.json")?,
            },
        )
        .with_external_initializer("model_q4.onnx_data".into(), read("model_q4.onnx_data")?);
        model.output_key = Some(OutputKey::ByName("sentence_embedding"));
        let model = TextEmbedding::try_new_from_user_defined(
            model,
            InitOptionsUserDefined::new()
                .with_max_length(512)
                .with_intra_threads(4),
        )
        .map_err(failure)?;
        check()?;
        Ok(Self(Mutex::new(model)))
    }

    fn batch(&self, texts: &[String], check: Checkpoint<'_>) -> Result<Vec<Vec<f32>>, ToolError> {
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(4) {
            let output = lock(&self.0, check)?
                .embed(batch, Some(4))
                .map_err(failure)?;
            check()?;
            vectors.extend(output);
        }
        if vectors.len() != texts.len()
            || vectors
                .iter()
                .any(|v| v.len() != 768 || v.iter().any(|x| !x.is_finite()))
        {
            return Err(failure("invalid embedding output"));
        }
        Ok(vectors)
    }
}

impl Embedder for LocalEmbedder {
    fn name(&self) -> &'static str {
        NAME
    }
    fn embed(&self, text: &str, check: Checkpoint<'_>) -> Result<Vec<f32>, ToolError> {
        Ok(self
            // 2026-09-29 calibration: forcing "code retrieval" made a cooking
            // question resemble recipe UI source (.557). The model-card default
            // search task retains natural query intent (.291), below our floor.
            .batch(&[format!("task: search result | query: {text}")], check)?
            .remove(0))
    }
    fn embed_documents(
        &self,
        path: &str,
        chunks: &[(usize, String)],
        check: Checkpoint<'_>,
    ) -> Result<Vec<Vec<f32>>, ToolError> {
        // Model-card task prefixes distinguish natural-language code queries
        // from documents. Paths are titles; source remains entirely local.
        self.batch(
            &chunks
                .iter()
                .map(|(_, text)| format!("title: {path} | text: {text}"))
                .collect::<Vec<_>>(),
            check,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_for_model_is_cancellable_and_corrupt_assets_fail_closed() {
        let mutex = Mutex::new(());
        let _held = mutex.lock().unwrap();
        let attempts = std::cell::Cell::new(0);
        let result = lock(&mutex, &|| {
            attempts.set(attempts.get() + 1);
            if attempts.get() == 2 {
                Err(failure("cancelled"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("model_q4.onnx"), b"corrupt").unwrap();
        assert!(
            matches!(LocalEmbedder::open(dir.path(), &|| Ok(())), Err(ToolError::Exec(message)) if message.contains("integrity check"))
        );
    }
}

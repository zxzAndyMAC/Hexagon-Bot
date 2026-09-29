//! 本地相似检索（code-search-and-subagent 票 02）：自然语言 → 路径/行号/短摘。
//!
//! 硬约束的实现选择：
//! - **本地性**：安装固定校验的多语言模型后本地推理，无模型时保留字符
//!   n-gram 引擎；结果明示 engine。运行期不下载、不上传源码、不调用付费
//!   接口。字符引擎只比较重叠，不理解跨语言含义；两者均优先保留字面命中。
//! - **增量**：code_files 记「嵌入器名+内容哈希」——嵌入器实现换了即全量
//!   重建（旧向量对新向量是噪声），文件没变则整文件跳过，不重嵌未动块。
//! - **存储**：向量随项目库（state.db），天然 per-project 不共享；
//!   文件路径表与块表一对多，删文件连带删块。
//! - **无命中**：调用方负责回退到 fs_grep（spec 明确「语义搜不到回退文本
//!   搜索」），这里不藏自动回退——命中为空是诚实信号。

use crate::db::Db;
use crate::tools::fnv64;
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

mod model;
pub use model::LocalEmbedder;

/// 哈希嵌入维度：256 桶 × 4B = 1KB/块。再大选不起（4k 文件仓 ≈ 几十万块），
/// 再小撞桶率伤召回。不与任何神经嵌入器维度对齐——签名哈希自报家门。
const DIMS: usize = 256;
/// 一块的行数与步距：48 行窗口、40 行步进（约 17% 重叠，边界命中不丢）。
const CHUNK_LINES: usize = 48;
const CHUNK_STEP: usize = 40;
/// 索引收录单文件上限：与文本搜索同口径。
const INDEX_FILE_BYTES: u64 = 256 * 1024;

/// 嵌入器接缝（spec：「嵌入器用替身」）：测试可注入查表桩。
/// 实现必须确定性——同文本同向量，否则索引哈希对不上。
pub type Checkpoint<'a> = &'a dyn Fn() -> Result<(), crate::tools::ToolError>;

pub trait Embedder: Send + Sync {
    /// 实现名进哈希签名：换实现即全量重建。
    fn name(&self) -> &'static str;
    fn embed(&self, text: &str, check: Checkpoint<'_>)
        -> Result<Vec<f32>, crate::tools::ToolError>;
    fn embed_documents(
        &self,
        _path: &str,
        chunks: &[(usize, String)],
        check: Checkpoint<'_>,
    ) -> Result<Vec<Vec<f32>>, crate::tools::ToolError> {
        chunks
            .iter()
            .map(|(_, text)| {
                check()?;
                self.embed(text, check)
            })
            .collect()
    }
}

/// 默认本地嵌入器：字符 3-gram 签名哈希到 256 维，正负号消偏。
/// 无外部依赖、无网络、中英混排可用——「小而本地」的最便宜答案。
pub struct HashEmbedder;

impl Embedder for HashEmbedder {
    fn name(&self) -> &'static str {
        "hash-ngram-v1"
    }
    fn embed(
        &self,
        text: &str,
        check: Checkpoint<'_>,
    ) -> Result<Vec<f32>, crate::tools::ToolError> {
        check()?;
        let mut v = vec![0f32; DIMS];
        let norm: String = text
            .to_lowercase()
            .chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .collect();
        let chars: Vec<char> = norm.chars().collect();
        if chars.len() < 3 {
            // 短文本退化：整串哈希成单点，至少能精确召回
            if !chars.is_empty() {
                let h = fnv64(&norm);
                v[(h as usize) % DIMS] = 1.0;
            }
        } else {
            for w in chars.windows(3) {
                let g: String = w.iter().collect();
                let h = fnv64(&g);
                let bucket = (h as usize) % DIMS;
                // 第二哈希位定正负——签名哈希把碰撞变成噪声而非系统性偏向。
                v[bucket] += if h & (1 << 63) != 0 { -1.0 } else { 1.0 };
            }
        }
        let norm2: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm2 > 0.0 {
            for x in &mut v {
                *x /= norm2;
            }
        }
        Ok(v)
    }
}

/// ctx.embedder 为 None 时惰性加载共享本地模型；未安装时用字符引擎。
pub fn default_embedder(
    check: Checkpoint<'_>,
) -> Result<Arc<dyn Embedder>, crate::tools::ToolError> {
    model::default_embedder(check)
}

/// 把文本切成 (line_start, chunk_text)。行号 1-based。
fn chunks_of(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let end = (i + CHUNK_LINES).min(lines.len());
        let body = lines[i..end].join("\n");
        if !body.trim().is_empty() {
            out.push((i + 1, body));
        }
        if end == lines.len() {
            break;
        }
        i += CHUNK_STEP;
    }
    out
}

/// 向量 → BLOB（LE f32 平铺）。
fn pack(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}
fn unpack(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

// A15 review (2026-09-29): indexing, literal evidence and excerpts must read
// the same text. Strict UTF-8 in only one path silently lost indexed identifiers.
fn indexed_text(root: &Path, rel: &str) -> Option<String> {
    let path = crate::tools::agent_readable_repo_path(root, rel).ok()?;
    if path.metadata().ok()?.len() > INDEX_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    // Bound the read itself as well: an external editor can grow the file after
    // metadata was checked. Keep the existing 8 KiB binary probe and lossy decode.
    std::fs::File::open(path)
        .ok()?
        .take(INDEX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty()
        || bytes.len() as u64 > INDEX_FILE_BYTES
        || bytes[..bytes.len().min(8192)].contains(&0)
    {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// 增量刷新索引：新/变文件重嵌，消失文件连带清块，未动文件整文件跳过。
/// 返回本次重嵌的文件数（事件/结果载荷的可观测面）。
pub fn refresh(
    db: &Db,
    root: &Path,
    embedder: &dyn Embedder,
    check: Checkpoint<'_>,
) -> Result<usize, crate::tools::ToolError> {
    let files: Vec<String> = crate::search::repo_files(root)
        .into_iter()
        .take(crate::search::INDEX_FILE_CAP)
        .collect();
    // Audit A15 (2026-09-28): per-row commits and a shrinking Vec made refresh
    // needlessly expensive. Keep content hashing: mtime alone misses replacements.
    let mut indexed = 0usize;
    let mut stale: std::collections::HashSet<String> = db
        .conn()
        .prepare("SELECT path FROM code_files")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for rel in &files {
        check()?;
        let Some(text) = indexed_text(root, rel) else {
            continue;
        };
        stale.remove(rel);
        let sig = format!("{}:{:016x}", embedder.name(), fnv64(&text));
        let cur: Option<String> = db
            .conn()
            .query_row("SELECT hash FROM code_files WHERE path=?1", [rel], |r| {
                r.get(0)
            })
            .ok();
        if cur.as_deref() == Some(sig.as_str()) {
            continue;
        }
        let chunks = chunks_of(&text);
        let vectors = embedder.embed_documents(rel, &chunks, check)?;
        if vectors.len() != chunks.len() {
            return Err(crate::tools::ToolError::Exec(
                "embedding count mismatch".into(),
            ));
        }
        check()?;
        // A15 review 2026-09-29: an editor can replace source while inference
        // runs. Never commit that old vector as though it described new text.
        if indexed_text(root, rel).as_deref() != Some(text.as_str()) {
            return Err(crate::tools::ToolError::Exec(
                "source changed during indexing; retry search".into(),
            ));
        }
        // A15 neural indexing can take seconds per file. Infer before taking
        // a write transaction, then commit each complete file atomically; a
        // failed inference keeps that file's previous index for a later retry.
        let tx = db.conn().unchecked_transaction()?;
        db.conn()
            .execute("DELETE FROM code_chunks WHERE path=?1", [rel])?;
        db.conn().execute(
            "INSERT OR REPLACE INTO code_files(path, hash) VALUES(?1, ?2)",
            rusqlite::params![rel, sig],
        )?;
        for (idx, ((line_start, _), vector)) in chunks.into_iter().zip(vectors).enumerate() {
            db.conn().execute(
                "INSERT INTO code_chunks(path, idx, line_start, vec) VALUES(?1,?2,?3,?4)",
                rusqlite::params![rel, idx as i64, line_start as i64, pack(&vector)],
            )?;
        }
        tx.commit()?;
        indexed += 1;
    }
    // 仓里已消失的文件：行与块一起清（不存在不报错——幂等收尾）。
    let tx = db.conn().unchecked_transaction()?;
    for p in stale {
        db.conn()
            .execute("DELETE FROM code_files WHERE path=?1", [&p])?;
        db.conn()
            .execute("DELETE FROM code_chunks WHERE path=?1", [&p])?;
    }
    tx.commit()?;
    Ok(indexed)
}

// Audit A15, 50-query synthetic calibration (2026-09-28): unrelated maxima
// reached .262; literal positives started at .308. A .30 floor only applies to
// this hash engine, not injected embedders. Gemma's separate .35 floor was
// checked on the frozen source corpus: unrelated max .291, weakest recovered
// Chinese target .373. These are development queries, not a general guarantee.
// False negatives cost a literal-search fallback; false positives waste
// inspection. Neither cutoff is semantic confidence.
fn candidate_score(engine: &str, score: f32) -> bool {
    let floor = if engine == "hash-ngram-v1" {
        0.30
    } else if engine == model::NAME {
        0.35
    } else {
        0.0
    };
    score.is_finite() && score > floor
}

/// 本地相似检索：默认引擎优先保留字面命中文件，再按全块内积取 top-k。
/// 字面命中定位实际行；近似命中定位最佳块。分数仍为块余弦，不是置信度。
pub fn query(
    db: &Db,
    root: &Path,
    embedder: &dyn Embedder,
    q: &str,
    cap: usize,
    check: Checkpoint<'_>,
) -> Result<Vec<Value>, crate::tools::ToolError> {
    if q.trim().is_empty() || cap == 0 {
        return Ok(Vec::new());
    }
    // A15 real-source measurement (2026-09-29): 48-line normalization drowned
    // out literal identifiers (.065 vs .783 alone). Lowering the global floor
    // would also admit hash collisions. Keep exact evidence ahead of similarity
    // and point the excerpt at the actual matching line, not the chunk's start.
    check()?;
    let qv = embedder.embed(q, check)?;
    check()?;
    let mut literal_lines = std::collections::HashMap::new();
    let mut valid = std::collections::HashMap::new();
    let mut files = db.conn().prepare("SELECT path, hash FROM code_files")?;
    for row in files.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        check()?;
        let (path, hash) = row?;
        let Some(text) = indexed_text(root, &path) else {
            continue;
        };
        if hash != format!("{}:{:016x}", embedder.name(), fnv64(&text)) {
            continue;
        }
        if matches!(embedder.name(), "hash-ngram-v1" | model::NAME) {
            if let Some(offset) = text.find(q) {
                let line = text[..offset].bytes().filter(|&b| b == b'\n').count() + 1;
                literal_lines.insert(path.clone(), line as i64);
            }
        }
        valid.insert(path, hash);
    }
    let mut st = db
        .conn()
        .prepare("SELECT path, line_start, vec FROM code_chunks")?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    // A15: select distinct files before top-k. A large file's first 24 chunks
    // previously crowded all other files out. Bound sorting to the requested k.
    let mut seen: std::collections::HashMap<String, (f32, i64)> = Default::default();
    for row in rows {
        let (path, line_start, blob) = row?;
        check()?;
        if !valid.contains_key(&path) {
            continue;
        }
        let v = unpack(&blob);
        let dot: f32 = qv.iter().zip(v.iter()).map(|(a, b)| a * b).sum();
        if literal_lines.contains_key(&path) || candidate_score(embedder.name(), dot) {
            seen.entry(path)
                .and_modify(|e| {
                    if dot > e.0 || (dot == e.0 && line_start < e.1) {
                        *e = (dot, line_start);
                    }
                })
                .or_insert((dot, line_start));
        }
    }
    let mut hits: Vec<_> = seen.into_iter().collect();
    let order = |a: &(String, (f32, i64)), b: &(String, (f32, i64))| {
        literal_lines
            .contains_key(&b.0)
            .cmp(&literal_lines.contains_key(&a.0))
            .then_with(|| b.1 .0.total_cmp(&a.1 .0))
            .then_with(|| a.0.cmp(&b.0))
    };
    if cap < hits.len() {
        hits.select_nth_unstable_by(cap, order);
        hits.truncate(cap);
    }
    hits.sort_by(order);
    Ok(hits
        .into_iter()
        .filter_map(|(path, (score, line_start))| {
            let text = indexed_text(root, &path)?;
            if valid.get(&path)? != &format!("{}:{:016x}", embedder.name(), fnv64(&text)) {
                return None;
            }
            let literal = literal_lines.get(&path);
            let line_start = literal.copied().unwrap_or(line_start);
            Some(json!({
                "path": path,
                "line": line_start,
                "excerpt": excerpt(&text, line_start),
                "score": format!("{score:.3}"),
                "match": if literal.is_some() { "literal" } else { "similarity" },
            }))
        })
        .collect())
}

/// 命中块的短摘：从块首行起取非空行拼到 ~240 字符。
fn excerpt(text: &str, line_start: i64) -> String {
    let mut out = String::new();
    for line in text
        .lines()
        .skip((line_start as usize).saturating_sub(1))
        .take(8)
    {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str(" ⏎ ");
        }
        out.push_str(t);
        if out.chars().count() > 240 {
            out = out.chars().take(240).collect();
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 查表嵌入桩（spec：嵌入器用替身——断言命中与回退，不断言维度/算法）。
    /// 词→固定基向量：命中与否由「词是否同现」决定，完全确定。
    struct Dict(Vec<(&'static str, usize)>);
    impl Embedder for Dict {
        fn name(&self) -> &'static str {
            "dict-stub"
        }
        fn embed(
            &self,
            text: &str,
            check: Checkpoint<'_>,
        ) -> Result<Vec<f32>, crate::tools::ToolError> {
            check()?;
            let mut v = vec![0f32; DIMS];
            for (w, i) in &self.0 {
                if text.contains(w) {
                    v[i % DIMS] = 1.0;
                }
            }
            Ok(v)
        }
    }

    fn fixture() -> (Db, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let w = |rel: &str, body: &str| {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "src/orchestra.rs",
            "/// 裁决回路在此\nfn adjudicate() { /* ruling logic */ }\n",
        );
        w("src/turn.rs", "fn run() { /* streaming loop */ }\n");
        w("README.md", "project notes\n");
        let db = Db::open_in_memory().unwrap();
        (db, dir)
    }

    #[test]
    fn semantic_hit_returns_path_line_excerpt() {
        let (db, dir) = fixture();
        let emb = Dict(vec![("ruling", 1), ("adjudicate", 1), ("裁决", 1)]);
        refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap();
        let hits = query(&db, dir.path(), &emb, "裁决是怎么判定的", 10, &|| Ok(())).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["path"], "src/orchestra.rs");
        assert_eq!(hits[0]["line"], 1);
        assert!(hits[0]["excerpt"].as_str().unwrap().contains("裁决回路"));
    }

    #[test]
    fn refresh_is_incremental() {
        let (db, dir) = fixture();
        let emb = Dict(vec![("streaming", 2), ("裁决", 1)]);
        let n1 = refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap();
        assert_eq!(n1, 3);
        // 未动文件不重嵌
        assert_eq!(refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap(), 0);
        // 改一个 → 只重嵌一个
        std::fs::write(dir.path().join("README.md"), "changed\n").unwrap();
        assert_eq!(refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap(), 1);
        // 删一个 → 块清掉
        std::fs::remove_file(dir.path().join("src/turn.rs")).unwrap();
        refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap();
        let gone = query(&db, dir.path(), &emb, "streaming", 10, &|| Ok(())).unwrap();
        assert!(!gone.iter().any(|h| h["path"] == "src/turn.rs"));
    }

    #[test]
    fn no_hit_is_empty_signal_for_fallback() {
        let (db, dir) = fixture();
        let emb = Dict(vec![("ruling", 1)]);
        refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap();
        assert!(
            query(&db, dir.path(), &emb, "unrelated needle", 10, &|| Ok(()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn cap_is_enforced() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..8 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), "magic\n").unwrap();
        }
        let db = Db::open_in_memory().unwrap();
        let emb = Dict(vec![("magic", 1)]);
        refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap();
        assert_eq!(
            query(&db, dir.path(), &emb, "magic", 3, &|| Ok(()))
                .unwrap()
                .len(),
            3
        );
    }
    proptest::proptest! {
        #[test]
        fn weak_hash_scores_always_require_fallback(score in -1000f32..=0.30f32) {
            proptest::prop_assert!(!candidate_score("hash-ngram-v1", score));
        }
        #[test]
        fn weak_multilingual_scores_always_require_fallback(score in -1000f32..=0.35f32) {
            proptest::prop_assert!(!candidate_score(model::NAME, score));
        }
        #[test]
        fn multilingual_candidates_are_finite_and_monotonic(score in proptest::prelude::any::<f32>(), extra in 0.0f32..1.0) {
            if candidate_score(model::NAME, score) {
                proptest::prop_assert!(score.is_finite());
                if (score + extra).is_finite() {
                    proptest::prop_assert!(candidate_score(model::NAME, score + extra));
                }
            }
        }
        #[test]
        fn raising_similarity_preserves_a_candidate(a in 0.31f32..1.0, extra in 0.0f32..1.0) {
            proptest::prop_assert!(candidate_score("hash-ngram-v1", a));
            proptest::prop_assert!(candidate_score("hash-ngram-v1", a+extra));
        }
    }
}

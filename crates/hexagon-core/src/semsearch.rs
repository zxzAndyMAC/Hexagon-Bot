//! 本地相似检索（code-search-and-subagent 票 02）：自然语言 → 路径/行号/短摘。
//!
//! 硬约束的实现选择：
//! - **本地性**：字符 n-gram 引擎，无模型权重、运行期下载或网络调用。
//!   只比较文本重叠，不理解跨语言含义；中文探索由 Agent 提出候选标识词，
//!   使用 fs_find/fs_grep/fs_read 定位并验证，不能把字符相似度当语义模型。
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
            for (index, w) in chars.windows(3).enumerate() {
                if index % 1024 == 0 {
                    check()?;
                }
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

/// Owner 2026-09-30: remove neural assets; legacy vectors rebuild via engine signatures.
pub fn default_embedder(
    check: Checkpoint<'_>,
) -> Result<Arc<dyn Embedder>, crate::tools::ToolError> {
    check()?;
    Ok(Arc::new(HashEmbedder))
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
enum IndexedText {
    Text(String),
    Large,
    Binary,
    Empty,
}

fn indexed_text(root: &Path, rel: &str) -> Result<IndexedText, crate::tools::ToolError> {
    let path = crate::tools::agent_readable_repo_path(root, rel)?;
    if path.metadata()?.len() > INDEX_FILE_BYTES {
        return Ok(IndexedText::Large);
    }
    let mut bytes = Vec::new();
    // Bound growth after metadata; IO errors are errors, never empty search.
    std::fs::File::open(path)?
        .take(INDEX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > INDEX_FILE_BYTES {
        return Ok(IndexedText::Large);
    }
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return Ok(IndexedText::Binary);
    }
    if bytes.is_empty() {
        return Ok(IndexedText::Empty);
    }
    Ok(IndexedText::Text(
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

#[derive(Default, serde::Serialize)]
pub struct IndexCoverage {
    pub entries_visited: usize,
    pub files_discovered: usize,
    pub files_searched: usize,
    pub skipped_large: usize,
    pub skipped_binary: usize,
    pub excluded_by_policy: usize,
    pub skipped_changed: usize,
    pub skipped_unreadable: usize,
    pub reason: Option<&'static str>,
    pub truncated: bool,
    pub complete: bool,
}
impl IndexCoverage {
    pub fn finish(&mut self, changed: usize, hit_limit: bool) {
        self.skipped_changed = changed;
        self.reason = self
            .reason
            .or(if hit_limit { Some("hit_limit") } else { None });
        self.truncated = self.reason.is_some();
        self.complete = !self.truncated
            && self.skipped_large == 0
            && self.skipped_binary == 0
            && self.excluded_by_policy == 0
            && self.skipped_changed == 0
            && self.skipped_unreadable == 0;
    }
}

pub struct RefreshResult {
    pub indexed_files: usize,
    pub coverage: IndexCoverage,
    pub paths: Vec<String>,
}

/// 增量刷新索引：新/变文件重嵌，消失文件连带清块，未动文件整文件跳过。
/// 返回本次重嵌的文件数（事件/结果载荷的可观测面）。
pub fn refresh(
    db: &Db,
    root: &Path,
    embedder: &dyn Embedder,
    check: Checkpoint<'_>,
) -> Result<RefreshResult, crate::tools::ToolError> {
    let discovery = crate::search::repo_files_checked(root, crate::search::INDEX_FILE_CAP, check)?;
    let mut coverage = IndexCoverage {
        entries_visited: discovery.entries_visited,
        files_discovered: discovery.paths.len(),
        excluded_by_policy: discovery.excluded_by_policy,
        reason: discovery.reason,
        ..Default::default()
    };
    // Audit A15 (2026-09-28): per-row commits and a shrinking Vec made refresh
    // needlessly expensive. Keep content hashing: mtime alone misses replacements.
    let mut indexed = 0usize;
    let mut stale: std::collections::HashSet<String> = db
        .conn()
        .prepare("SELECT path FROM code_files")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for rel in &discovery.paths {
        check()?;
        let text = match indexed_text(root, rel)? {
            IndexedText::Text(text) => text,
            IndexedText::Large => {
                coverage.skipped_large += 1;
                continue;
            }
            IndexedText::Binary => {
                coverage.skipped_binary += 1;
                continue;
            }
            IndexedText::Empty => continue,
        };
        coverage.files_searched += 1;
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
        if !matches!(indexed_text(root, rel)?, IndexedText::Text(current) if current == text) {
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
    // R7 (2026-10-06): omission by a cap is not evidence of deletion.
    // ponytail: partial walks retain old cache rows; complete walks prune them.
    // Query always re-reads/hash-checks rows; retaining a row never trusts old text.
    if discovery.reason.is_none() {
        check()?;
        let tx = db.conn().unchecked_transaction()?;
        for p in stale {
            check()?;
            db.conn()
                .execute("DELETE FROM code_files WHERE path=?1", [&p])?;
            db.conn()
                .execute("DELETE FROM code_chunks WHERE path=?1", [&p])?;
        }
        check()?;
        tx.commit()?;
    }
    coverage.finish(0, false);
    Ok(RefreshResult {
        indexed_files: indexed,
        coverage,
        paths: discovery.paths,
    })
}

// Audit A15, 50-query synthetic calibration (2026-09-28): unrelated maxima
// reached .262; literal positives started at .308. A .30 floor only applies to
// this hash engine, not injected embedders. Development calibration is not a
// semantic correctness guarantee.
// False negatives cost a literal-search fallback; false positives waste
// inspection. Neither cutoff is semantic confidence.
fn candidate_score(engine: &str, score: f32) -> bool {
    let floor = if engine == "hash-ngram-v1" { 0.30 } else { 0.0 };
    score.is_finite() && score > floor
}

/// 本地相似检索：默认引擎优先保留字面命中文件，再按全块内积取 top-k。
/// 字面命中定位实际行；近似命中定位最佳块。分数仍为块余弦，不是置信度。
pub struct QueryResult {
    pub hits: Vec<Value>,
    pub skipped_changed: usize,
    pub skipped_unreadable: usize,
    pub truncated: bool,
}

pub fn query(
    db: &Db,
    root: &Path,
    embedder: &dyn Embedder,
    q: &str,
    cap: usize,
    check: Checkpoint<'_>,
    paths: &[String],
) -> Result<QueryResult, crate::tools::ToolError> {
    check()?;
    if q.trim().is_empty() || cap == 0 {
        return Ok(QueryResult {
            hits: Vec::new(),
            skipped_changed: 0,
            skipped_unreadable: 0,
            truncated: false,
        });
    }
    // A15 real-source measurement (2026-09-29): 48-line normalization drowned
    // out literal identifiers (.065 vs .783 alone). Lowering the global floor
    // would also admit hash collisions. Keep exact evidence ahead of similarity
    // and point the excerpt at the actual matching line, not the chunk's start.
    check()?;
    let qv = embedder.embed(q, check)?;
    check()?;
    // R7: retained cache rows outside this walk (new ignores/caps) are not
    // evidence for this query. Only current discovery can select candidate paths.
    let paths: std::collections::HashSet<&str> = paths.iter().map(String::as_str).collect();
    let mut skipped_changed = 0;
    let mut skipped_unreadable = 0;
    let mut literal_lines = std::collections::HashMap::new();
    let mut valid = std::collections::HashMap::new();
    let mut files = db.conn().prepare("SELECT path, hash FROM code_files")?;
    for row in files.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        check()?;
        let (path, hash) = row?;
        if !paths.contains(path.as_str()) {
            continue;
        }
        let text = match indexed_text(root, &path) {
            Ok(IndexedText::Text(text)) => text,
            Err(crate::tools::ToolError::Io(_)) => {
                skipped_unreadable += 1;
                continue;
            }
            Err(error) => return Err(error),
            Ok(_) => {
                skipped_changed += 1;
                continue;
            }
        };

        if hash != format!("{}:{:016x}", embedder.name(), fnv64(&text)) {
            skipped_changed += 1;
            continue;
        }
        if embedder.name() == "hash-ngram-v1" {
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
    let truncated = cap < hits.len();
    if truncated {
        hits.select_nth_unstable_by(cap, order);
        hits.truncate(cap);
    }
    hits.sort_by(order);
    let mut results = Vec::new();
    for (path, (score, line_start)) in hits {
        check()?;
        let text = match indexed_text(root, &path) {
            Ok(IndexedText::Text(text)) => text,
            Err(crate::tools::ToolError::Io(_)) => {
                skipped_unreadable += 1;
                continue;
            }
            Err(error) => return Err(error),
            Ok(_) => {
                skipped_changed += 1;
                continue;
            }
        };

        if valid.get(&path) != Some(&format!("{}:{:016x}", embedder.name(), fnv64(&text))) {
            skipped_changed += 1;
            continue;
        }
        let literal = literal_lines.get(&path);
        let line_start = literal.copied().unwrap_or(line_start);
        results.push(json!({
            "path": path, "line": line_start, "excerpt": excerpt(&text, line_start),
            "score": format!("{score:.3}"),
            "match": if literal.is_some() { "literal" } else { "similarity" },
        }));
    }
    check()?;
    Ok(QueryResult {
        hits: results,
        skipped_changed,
        skipped_unreadable,
        truncated,
    })
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

    // Competitor benchmark R7 (2026-10-06): discovery used to run before
    // the first checkpoint; empty repositories could report success on stop.
    #[test]
    fn stopped_empty_discovery_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let result = refresh(&db, dir.path(), &HashEmbedder, &|| {
            Err(crate::tools::ToolError::Exec("fixture stopped".into()))
        });
        assert!(
            result.is_err(),
            "stop must never become successful empty search"
        );
    }

    #[test]
    fn invalid_ignore_discovery_is_an_error_and_preserves_index() {
        let (db, dir) = fixture();
        refresh(&db, dir.path(), &HashEmbedder, &|| Ok(())).unwrap();
        let before: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM code_files", [], |r| r.get(0))
            .unwrap();
        std::fs::write(dir.path().join(".gitignore"), "[z-a]\n").unwrap();
        let result = refresh(&db, dir.path(), &HashEmbedder, &|| Ok(()));
        assert!(
            result.is_err(),
            "broken ignore rules must not be silently accepted"
        );
        let after: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM code_files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn missing_root_is_an_error_and_preserves_index() {
        let (db, dir) = fixture();
        refresh(&db, dir.path(), &HashEmbedder, &|| Ok(())).unwrap();
        let missing = dir.path().join("missing");
        assert!(refresh(&db, &missing, &HashEmbedder, &|| Ok(())).is_err());
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM code_files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 3,
            "failed discovery must not delete an existing index"
        );
    }

    #[test]
    fn semantic_hit_returns_path_line_excerpt() {
        let (db, dir) = fixture();
        let emb = Dict(vec![("ruling", 1), ("adjudicate", 1), ("裁决", 1)]);
        refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap();
        let hits = query(
            &db,
            dir.path(),
            &emb,
            "裁决是怎么判定的",
            10,
            &|| Ok(()),
            &crate::search::repo_files(dir.path()),
        )
        .unwrap()
        .hits;
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
        assert_eq!(n1.indexed_files, 3);
        // 未动文件不重嵌
        assert_eq!(
            refresh(&db, dir.path(), &emb, &|| Ok(()))
                .unwrap()
                .indexed_files,
            0
        );
        // 改一个 → 只重嵌一个
        std::fs::write(dir.path().join("README.md"), "changed\n").unwrap();
        assert_eq!(
            refresh(&db, dir.path(), &emb, &|| Ok(()))
                .unwrap()
                .indexed_files,
            1
        );
        // 删一个 → 块清掉
        std::fs::remove_file(dir.path().join("src/turn.rs")).unwrap();
        refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap();
        let gone = query(
            &db,
            dir.path(),
            &emb,
            "streaming",
            10,
            &|| Ok(()),
            &crate::search::repo_files(dir.path()),
        )
        .unwrap()
        .hits;
        assert!(!gone.iter().any(|h| h["path"] == "src/turn.rs"));
    }

    #[test]
    fn no_hit_is_empty_signal_for_fallback() {
        let (db, dir) = fixture();
        let emb = Dict(vec![("ruling", 1)]);
        refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap();
        assert!(query(
            &db,
            dir.path(),
            &emb,
            "unrelated needle",
            10,
            &|| Ok(()),
            &crate::search::repo_files(dir.path())
        )
        .unwrap()
        .hits
        .is_empty());
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
            query(
                &db,
                dir.path(),
                &emb,
                "magic",
                3,
                &|| Ok(()),
                &crate::search::repo_files(dir.path())
            )
            .unwrap()
            .hits
            .len(),
            3
        );
    }
    #[test]
    fn index_coverage_exposes_skipped_content_and_capped_hits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("large.rs"),
            vec![b'x'; INDEX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::write(dir.path().join("binary.rs"), b"magic\0").unwrap();
        std::fs::write(dir.path().join(".env"), "magic secret").unwrap();
        for name in ["one.rs", "two.rs"] {
            std::fs::write(dir.path().join(name), "magic").unwrap();
        }
        let db = Db::open_in_memory().unwrap();
        let emb = Dict(vec![("magic", 1)]);
        let mut result = refresh(&db, dir.path(), &emb, &|| Ok(())).unwrap();
        assert_eq!(result.coverage.skipped_large, 1);
        assert_eq!(result.coverage.skipped_binary, 1);
        assert_eq!(result.coverage.excluded_by_policy, 1);
        assert!(!result.coverage.complete);
        let queried = query(
            &db,
            dir.path(),
            &emb,
            "magic",
            1,
            &|| Ok(()),
            &crate::search::repo_files(dir.path()),
        )
        .unwrap();
        assert_eq!(queried.hits.len(), 1);
        result
            .coverage
            .finish(queried.skipped_changed, queried.truncated);
        assert_eq!(result.coverage.reason, Some("hit_limit"));
        assert!(!result.coverage.complete);
    }

    #[test]
    fn query_read_failure_is_not_complete_empty_success() {
        struct Removing(std::path::PathBuf);
        impl Embedder for Removing {
            fn name(&self) -> &'static str {
                "hash-ngram-v1"
            }
            fn embed(
                &self,
                text: &str,
                check: Checkpoint<'_>,
            ) -> Result<Vec<f32>, crate::tools::ToolError> {
                std::fs::remove_file(&self.0)?;
                HashEmbedder.embed(text, check)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.rs");
        std::fs::write(&path, "needle").unwrap();
        let db = Db::open_in_memory().unwrap();
        refresh(&db, dir.path(), &HashEmbedder, &|| Ok(())).unwrap();
        let queried = query(
            &db,
            dir.path(),
            &Removing(path),
            "needle",
            1,
            &|| Ok(()),
            &crate::search::repo_files(dir.path()),
        )
        .unwrap();
        assert!(queried.hits.is_empty());
        assert_eq!(queried.skipped_unreadable, 1);
        let mut coverage = IndexCoverage {
            skipped_unreadable: queried.skipped_unreadable,
            ..Default::default()
        };
        coverage.finish(queried.skipped_changed, queried.truncated);
        assert!(!coverage.complete);
    }

    #[test]
    fn hash_embedding_checks_stop_inside_long_chunks() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let result = HashEmbedder.embed(&"identifier".repeat(20_000), &|| {
            if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 3 {
                Err(crate::tools::ToolError::Exec("embedding stopped".into()))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 4);
    }

    #[test]
    fn capped_refresh_keeps_unvisited_cached_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        // A formerly indexed source can become ignored while a later walk is
        // capped. Preserve its cache, but never select it for the current query.
        std::fs::write(
            dir.path().join("unvisited.rs"),
            "private_symbol_after_ignore",
        )
        .unwrap();
        refresh(&db, dir.path(), &HashEmbedder, &|| Ok(())).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "unvisited.rs\n").unwrap();
        for i in 0..crate::search::INDEX_FILE_CAP {
            std::fs::write(dir.path().join(format!("f{i}.rs")), "").unwrap();
        }
        let result = refresh(&db, dir.path(), &HashEmbedder, &|| Ok(())).unwrap();
        assert_eq!(result.coverage.reason, Some("file_limit"));
        let retained: bool = db
            .conn()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM code_files WHERE path='unvisited.rs')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(retained);
        let queried = query(
            &db,
            dir.path(),
            &HashEmbedder,
            "private_symbol_after_ignore",
            10,
            &|| Ok(()),
            &result.paths,
        )
        .unwrap();
        assert!(queried.hits.iter().all(|hit| hit["path"] != "unvisited.rs"));
    }

    proptest::proptest! {
        #[test]
        fn incomplete_coverage_never_claims_complete(large in 0usize..8, binary in 0usize..8,
            excluded in 0usize..8, changed in 0usize..8, unreadable in 0usize..8, file_cap in proptest::bool::ANY, hit_cap in proptest::bool::ANY) {
            let mut coverage = IndexCoverage {
                skipped_large: large, skipped_binary: binary, excluded_by_policy: excluded, skipped_unreadable: unreadable,
                reason: if file_cap { Some("file_limit") } else { None }, ..Default::default()
            };
            coverage.finish(changed, hit_cap);
            proptest::prop_assert_eq!(coverage.complete,
                large == 0 && binary == 0 && excluded == 0 && changed == 0 && unreadable == 0 && !file_cap && !hit_cap);
        }
        #[test]
        fn weak_hash_scores_always_require_fallback(score in -1000f32..=0.30f32) {
            proptest::prop_assert!(!candidate_score("hash-ngram-v1", score));
        }
        #[test]
        fn raising_similarity_preserves_a_candidate(a in 0.31f32..1.0, extra in 0.0f32..1.0) {
            proptest::prop_assert!(candidate_score("hash-ngram-v1", a));
            proptest::prop_assert!(candidate_score("hash-ngram-v1", a+extra));
        }
    }
}

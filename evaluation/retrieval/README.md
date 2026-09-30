# Local text-similarity calibration

Current status (2026-09-30): the optional neural engine and installer have been removed by owner request. The following neural measurements are historical, not current capabilities. Chinese source exploration uses the configured Agent and scoped file/search/read tools. `sem_search` retains explicitly lexical similarity only. No local weights are loaded, including from the former environment override. Saved neural indices rebuild on next refresh; user assets are not deleted.

`corpus.json` contains original synthetic source snippets and 50 development queries: 30 literal/identifier, 10 paraphrases, 10 unrelated. It is a calibration fixture, not a held-out semantic retrieval evaluation.

The Workbench regression checks distinct-file top-k, literal Recall@5 and unrelated-query fallback. Optional `HEXAGON_RETRIEVAL_BENCH=1` expands `local_similarity_benchmark` to 100/1000/4000 files and five cold runs each; `HEXAGON_RETRIEVAL_REPORT` names a local JSON output path. Reports include cold samples, hot p50/p95 and every query's expected paths and returned hits. The fixture uses debug code and in-memory SQLite. Do not infer durable-database or real-project quality from these timings.

2026-09-28 measured baseline: hot p95 17.34/159.77/956.81 ms. Conservative same-host regression ceilings are 40/350/2000 ms hot and 60/400/2500 ms cold p95. After optimization: hot p95 17.37/148.92/649.72 ms, literal recall 30/30 and unrelated rejection 10/10 at each size. Paraphrase recall is 0/10; this remains character similarity, not learned semantics. The hash-ngram-v1 cutoff 0.30 was calibrated against this development fixture and may reject useful low-overlap matches. Prefer exact text/filename search, and verify excerpts.

Content hashing remains enabled on every refresh to catch replacements regardless of size/mtime. A future learned embedder or filesystem-change cache needs its own quality/freshness evidence rather than inheriting these results.

## Real repository measurement (2026-09-29)

`real-project-queries.json` was frozen before measuring 573 tracked files under
`crates/hexagon-core/src` and `ui/src` at commit `44390f9`. It contains eight
identifiers, eight Chinese questions and eight unrelated queries. No threshold
or retrieval implementation was changed using these results.

| Query group | Local similarity | Literal fs_grep |
| --- | --- | --- |
| Identifiers: expected file found | 0/8 in top 5 | 8/8 within returned hits (up to 100 lines) |
| Natural language: expected file found | 0/8 in top 5 | 0/8 within returned hits |
| Unrelated: empty result | 8/8 | 8/8 |

The two result caps differ; this is a check of the existing exact-search fallback,
not a claim that their ranking metrics are interchangeable. All expected files
were present and below the 256 KiB search limit. This is one small, author-selected
repository sample, not evidence of general semantic quality. Prefer fs_grep for
known symbols. The synthetic calibration's literal success did not generalize to
these real source files; natural-language retrieval remains unverified for use as
the sole navigation tool. A model upgrade needs fresh queries and its own budget.

To reproduce, copy those tracked directories from the frozen commit into a
disposable directory, preserving relative paths and excluding `.hexagon` runtime
configuration (the measurement rejects project MCP configuration). Set `HEXAGON_RETRIEVAL_PROJECT`
to that directory, `HEXAGON_RETRIEVAL_CASES` to the absolute path of
`real-project-queries.json`, and `HEXAGON_RETRIEVAL_REPORT` to a JSON output path.
Run `cargo test -p hexagon-core local_similarity_real_project_measurement -- --nocapture`.
The test uses the Workbench tool registry with an in-memory database, reports
misses rather than treating them as execution failures, and does not call remote models.
Without these environment variables the optional measurement does not run.

## Literal recall repair (2026-09-29)

A Workbench regression reproduced a literal identifier disappearing inside a
48-line source chunk: its cosine dropped from .783 in a short declaration to
.065 with surrounding code. The default engine now retains case-sensitive literal
matches in indexed files ahead of approximate matches, deduplicates by file, and
points their excerpts at the first matching line. The `match` field distinguishes
`literal` from `similarity`; `score` remains chunk cosine and is not the sole sort
key. Other candidates keep the .30 floor; injected embedders keep their original
score selection and ranking.

Rerunning the same frozen 573-file, 24-query snapshot gives identifier target-file
Recall@5 **8/8** (previously 0/8), Chinese questions **0/8**, and unrelated empty
results **8/8**. These are now development regression queries, not an independent
held-out evaluation. Literal recall is repaired on this sample; semantic quality
is not. Exact matching uses the same 4000-file indexed scope and read policy,
including the 256 KiB limit, and rereads current text rather than persisting a
second text index. The original measurement above remains the historical baseline.

Five-round debug measurements on the same host, including refresh and the tool
call, remain within the predeclared conservative ceilings:

| Files | Hot p95 (ms) | Maximum cold run (ms) |
| --- | --- | --- |
| 100 | 18.80 | 30.92 |
| 1000 | 175.91 | 210.81 |
| 4000 | 784.02 | 1044.57 |

The 4000-file hot p95 is about 21% above the earlier 649.72 ms record. These were
not interleaved controlled runs, so the entire difference cannot be attributed
to the implementation, but the extra current-text scan has a cost. Literal
calibration recall remains 30/30 and unrelated empty results remain 10/10 at each
size. This is a recall improvement, not a speedup or a large-repository latency
claim.

## Retired multilingual experiment (2026-09-29)

Before its removal on 2026-09-30, the optional engine used EmbeddingGemma 300M
Q4 with pinned ONNX/tokenizer assets (about 219 MB). These historical measurements
are retained; they do not describe the current character-similarity engine.

| Query group | Hash after literal repair | Former Gemma Q4 |
| --- | --- | --- |
| Identifier target-file Recall@5 | 8/8 | 8/8 |
| Chinese target-file Recall@5 | 0/8 | 6/8 |
| Unrelated empty results | 8/8 | 8/8 |

The frozen development sample was the original 573-file snapshot. Misses remained
for “工具执行结果不确定以后怎样核对恢复” and “离开后回来在哪里看到待处理事项”.
Related alternate files were not counted as success. This was not independent
held-out evidence. The former native debug measurement took 546.65 seconds for
cold indexing; subsequent queries had median 283.19 ms and p95 286.98 ms. It did
not meet hash-engine latency ceilings or establish arbitrary-repository quality.

The neural runtime, installer, environment override and CI weight download are
now removed. Model-free Workbench regressions cover default engine selection,
persistence, literal matches, deletion, cancellation and source freshness. The
Agent's Chinese source-answer quality requires separate real-model evaluation;
these old scores do not transfer to it or to the remaining lexical tool.

Final cancellation/freshness implementation, five-round hash-engine regression:

| Files | Hot p95 (ms) | Maximum cold run (ms) |
| --- | --- | --- |
| 100 | 16.10 | 32.78 |
| 1000 | 154.97 | 244.27 |
| 4000 | 603.04 | 1011.08 |

All remain within the predeclared hash-engine ceilings; results are local debug
measurements, not a general speedup claim.

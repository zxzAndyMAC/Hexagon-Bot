# Local text-similarity calibration

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

## Offline multilingual engine (2026-09-29)

The optional learned engine is EmbeddingGemma 300M Q4, using the pinned
[ONNX community export](https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX/tree/5090578d9565bb06545b4552f76e6bc2c93e4a66).
Its assets use [Gemma terms](https://ai.google.dev/gemma/terms); weights are not
included in this repository. `scripts/install-retrieval-model.py` installs about
219 MB using only Python 3.9+ standard library. `--check` verifies without network.
The application validates asset sizes and SHA-256 again before native inference.
The model manifest pins graph, external weights and tokenizer together.

Runtime is Rust/ONNX CPU, four threads, at most four texts per inference batch,
512 tokens per 48-line chunk, 768-dimensional normalized embeddings. Query prefix
is the model-card default `task: search result | query: `; documents use their
relative path as title and source as text. Index signatures include model revision,
prefix and token limit. There is no inference downloader, Python subprocess,
remote key, or paid request. `HEXAGON_EMBEDDING_MODEL_DIR` overrides the default
`~/.hexagon/models/embeddinggemma-300m-q4-v1`. Explicit missing/corrupt assets fail
with an error; an absent default installation retains `hash-ngram-v1`. The shared
loaded model lasts until process exit; restart after changing model installation.

Both built-in engines preserve literal priority. Learned candidates use a separate
.35 cosine floor, calibrated on the same development queries below. This is not a
confidence guarantee. An E5-small prototype was rejected because it recovered only
3/8 Chinese targets and admitted unrelated questions. No query-specific dictionary
or paid model was introduced.

Native Workbench measurement on the original frozen 573-file snapshot:

| Query group | Hash after literal repair | Gemma Q4 |
| --- | --- | --- |
| Identifier target-file Recall@5 | 8/8 | 8/8 |
| Chinese target-file Recall@5 | 0/8 | 6/8 |
| Unrelated empty results | 8/8 | 8/8 |

The original queries and expected files were retained. Misses remain for
“工具执行结果不确定以后怎样核对恢复” (`api.rs`) and
“离开后回来在哪里看到待处理事项” (`Timeline.tsx`). Related alternate files do not
count as success. These are development results, not held-out evidence of general
quality. A separate four-file English fixture checks four Chinese behaviors, two
unrelated questions, literal priority, persisted reuse after reopening and removal.
CI explicitly installs/verifies cached assets and runs this real-model regression;
the ordinary suite stays deterministic with an injected hash engine.

The native debug measurement took 546.65 seconds for its first query including
cold indexing; subsequent 23 queries had median 283.19 ms and p95 286.98 ms. This
initial ranking measurement preceded the cancellation/freshness checks; those
checks are covered separately and can add overhead. The neural cold path does
not satisfy the old hash-engine millisecond limits. Large files are still capped
at 256 KiB and repositories at 4000 indexed files; 512-token truncation can miss
code late in long chunks. No claim is made about arbitrary large-repository speed.

Search now checks cancellation between files, model lock waits and four-text
batches, and respects the earlier of the caller's deadline or a 15-minute invocation
budget. A native session initialization or currently executing batch finishes
before it can observe stop. Completed files remain reusable after a cancelled
refresh, so retry resumes indexing. Inference happens outside SQLite write
transactions; source changes during inference abort that file's commit. Query and
excerpt generation recheck current content against the indexed signature, avoiding
old-vector/new-excerpt pairs. Stop and inference failure are errors, not successful
empty-search responses.

For the real snapshot measurement, set `HEXAGON_EMBEDDING_MODEL_DIR` as well as the
three measurement variables above. Real-model regression:

```bash
python3 scripts/install-retrieval-model.py
HEXAGON_EMBEDDING_MODEL_DIR="$HOME/.hexagon/models/embeddinggemma-300m-q4-v1" \
  cargo test -p hexagon-core local_similarity_multilingual_model -- --nocapture
```

Final cancellation/freshness implementation, five-round hash-engine regression:

| Files | Hot p95 (ms) | Maximum cold run (ms) |
| --- | --- | --- |
| 100 | 16.10 | 32.78 |
| 1000 | 154.97 | 244.27 |
| 4000 | 603.04 | 1011.08 |

All remain within the predeclared hash-engine ceilings; results are local debug
measurements, not a general speedup claim.

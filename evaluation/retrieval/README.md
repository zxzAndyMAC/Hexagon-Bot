# Local text-similarity calibration

`corpus.json` contains original synthetic source snippets and 50 development queries: 30 literal/identifier, 10 paraphrases, 10 unrelated. It is a calibration fixture, not a held-out semantic retrieval evaluation.

The Workbench regression checks distinct-file top-k, literal Recall@5 and unrelated-query fallback. Optional `HEXAGON_RETRIEVAL_BENCH=1` expands `local_similarity_benchmark` to 100/1000/4000 files and five cold runs each; `HEXAGON_RETRIEVAL_REPORT` names a local JSON output path. Reports include cold samples, hot p50/p95 and every query's expected paths and returned hits. The fixture uses debug code and in-memory SQLite. Do not infer durable-database or real-project quality from these timings.

2026-09-28 measured baseline: hot p95 17.34/159.77/956.81 ms. Conservative same-host regression ceilings are 40/350/2000 ms hot and 60/400/2500 ms cold p95. After optimization: hot p95 17.37/148.92/649.72 ms, literal recall 30/30 and unrelated rejection 10/10 at each size. Paraphrase recall is 0/10; this remains character similarity, not learned semantics. The hash-ngram-v1 cutoff 0.30 was calibrated against this development fixture and may reject useful low-overlap matches. Prefer exact text/filename search, and verify excerpts.

Content hashing remains enabled on every refresh to catch replacements regardless of size/mtime. A future learned embedder or filesystem-change cache needs its own quality/freshness evidence rather than inheriting these results.

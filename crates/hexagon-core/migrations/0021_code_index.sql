-- 票 02 本地语义索引：文件哈希 + 块向量。随项目库走，天然 per-project。
-- code_files.hash = "{embedder}:{fnv64(content)}" —— 嵌入器实现变了即全量重建。

CREATE TABLE code_files (
    path TEXT PRIMARY KEY,
    hash TEXT NOT NULL
);

CREATE TABLE code_chunks (
    path TEXT NOT NULL,
    idx INTEGER NOT NULL,
    line_start INTEGER NOT NULL,
    vec BLOB NOT NULL,
    PRIMARY KEY (path, idx)
);

CREATE INDEX code_chunks_path ON code_chunks(path);

CREATE TABLE evaluation_runs (
    id TEXT PRIMARY KEY,
    task_json TEXT NOT NULL CHECK(json_valid(task_json)),
    result_json TEXT NOT NULL CHECK(json_valid(result_json))
);

CREATE TABLE experience_role_skills (
    project_id TEXT NOT NULL REFERENCES projects(id),
    role TEXT NOT NULL,
    skill TEXT NOT NULL,
    PRIMARY KEY(project_id,role),
    UNIQUE(project_id,skill)
);

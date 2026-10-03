CREATE TABLE IF NOT EXISTS cg_verified_executions (
    scope TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    trace TEXT NOT NULL,
    recorded_at BIGINT NOT NULL,
    execution_json TEXT NOT NULL,
    eligibility_json TEXT NOT NULL,
    PRIMARY KEY (scope, memory_id),
    UNIQUE (scope, trace)
);

CREATE INDEX IF NOT EXISTS cg_verified_executions_scope_time
    ON cg_verified_executions (scope, recorded_at, memory_id);

CREATE TABLE IF NOT EXISTS cg_memory_entries (
    scope TEXT NOT NULL,
    id TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK (revision > 0),
    eligibility_version BIGINT NOT NULL CHECK (eligibility_version > 0),
    state TEXT NOT NULL CHECK (state IN ('PENDING', 'VALIDATED', 'REJECTED', 'INVALIDATED', 'SUPERSEDED', 'FORGOTTEN')),
    source_snapshot TEXT NOT NULL,
    record_json TEXT NOT NULL,
    superseded_by TEXT,
    payload_forgotten BOOLEAN NOT NULL,
    PRIMARY KEY (scope, id)
);

CREATE UNIQUE INDEX IF NOT EXISTS cg_memory_current_snapshot
    ON cg_memory_entries (scope, source_snapshot)
    WHERE state <> 'FORGOTTEN';

CREATE TABLE IF NOT EXISTS cg_memory_decisions (
    scope TEXT NOT NULL,
    id TEXT NOT NULL,
    output_revision BIGINT NOT NULL CHECK (output_revision > 0),
    decision_json TEXT NOT NULL,
    PRIMARY KEY (scope, id, output_revision)
);

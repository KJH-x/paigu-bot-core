-- Migration: initial_schema
-- 排谷系统核心数据表

-- 1. users
CREATE TABLE IF NOT EXISTS users (
    user_id TEXT PRIMARY KEY,
    qq_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 2. groups
CREATE TABLE IF NOT EXISTS groups (
    group_id TEXT PRIMARY KEY,
    name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 3. rounds
CREATE TABLE IF NOT EXISTS rounds (
    round_id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL REFERENCES groups(group_id),
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    start_at TIMESTAMPTZ,
    end_at TIMESTAMPTZ,
    allow_cancel BOOLEAN NOT NULL DEFAULT TRUE,
    allow_modify BOOLEAN NOT NULL DEFAULT TRUE,
    default_timezone TEXT NOT NULL DEFAULT 'Asia/Shanghai',
    created_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_rounds_group_status ON rounds(group_id, status);

-- 4. items
CREATE TABLE IF NOT EXISTS items (
    item_id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL REFERENCES rounds(round_id),
    name TEXT NOT NULL,
    item_kind TEXT NOT NULL,
    unit_price_cents BIGINT NOT NULL,
    box_size INT,
    max_quantity INT,
    is_blind BOOLEAN NOT NULL DEFAULT FALSE,
    is_proxy_card BOOLEAN NOT NULL DEFAULT FALSE,
    sort_order INT NOT NULL DEFAULT 0,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_items_round ON items(round_id);

-- 5. item_aliases
CREATE TABLE IF NOT EXISTS item_aliases (
    alias_id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL REFERENCES rounds(round_id),
    item_id TEXT NOT NULL REFERENCES items(item_id),
    alias TEXT NOT NULL,
    weight INT NOT NULL DEFAULT 100
);
CREATE INDEX IF NOT EXISTS idx_aliases_round ON item_aliases(round_id);

-- 6. eligibility
CREATE TABLE IF NOT EXISTS eligibility (
    eligibility_id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL REFERENCES rounds(round_id),
    user_id TEXT NOT NULL,
    priority_type TEXT NOT NULL,
    priority_level INT NOT NULL,
    scope JSONB NOT NULL DEFAULT '{}',
    max_uses INT,
    used_count INT NOT NULL DEFAULT 0,
    valid_from TIMESTAMPTZ,
    valid_until TIMESTAMPTZ,
    note TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_eligibility_round ON eligibility(round_id);
CREATE INDEX IF NOT EXISTS idx_eligibility_user_round ON eligibility(user_id, round_id);

-- 7. raw_messages
CREATE TABLE IF NOT EXISTS raw_messages (
    raw_message_id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    qq_message_id TEXT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    text TEXT,
    images JSONB NOT NULL DEFAULT '[]',
    is_admin BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(group_id, qq_message_id)
);

-- 8. parsed_messages
CREATE TABLE IF NOT EXISTS parsed_messages (
    parsed_message_id TEXT PRIMARY KEY,
    raw_message_id TEXT NOT NULL REFERENCES raw_messages(raw_message_id),
    parser_version TEXT NOT NULL,
    prompt_hash TEXT NOT NULL,
    llm_raw_response TEXT NOT NULL,
    parsed_json JSONB NOT NULL,
    confidence NUMERIC,
    status TEXT NOT NULL,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 9. events
CREATE TABLE IF NOT EXISTS events (
    event_id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL REFERENCES rounds(round_id),
    group_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    raw_message_id TEXT,
    event_type TEXT NOT NULL,
    effective_at TIMESTAMPTZ NOT NULL,
    sequence BIGSERIAL NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_events_round ON events(round_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_events_sequence ON events(sequence);

-- 10. snapshots
CREATE TABLE IF NOT EXISTS snapshots (
    snapshot_id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL REFERENCES rounds(round_id),
    version BIGINT NOT NULL,
    allocation_json JSONB NOT NULL,
    settlement_json JSONB NOT NULL,
    public_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(round_id, version)
);

-- 11. replay_runs
CREATE TABLE IF NOT EXISTS replay_runs (
    replay_id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL,
    input_source TEXT NOT NULL,
    parser_mode TEXT NOT NULL,
    manifest_json JSONB NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

-- 12. replay_steps
CREATE TABLE IF NOT EXISTS replay_steps (
    replay_id TEXT NOT NULL,
    step_index BIGINT NOT NULL,
    round_id TEXT NOT NULL,
    event_id TEXT,
    raw_message_id TEXT,
    occurred_at TIMESTAMPTZ,
    step_summary JSONB NOT NULL,
    state_diff JSONB,
    decision_trace JSONB,
    warnings JSONB NOT NULL DEFAULT '[]',
    errors JSONB NOT NULL DEFAULT '[]',
    PRIMARY KEY (replay_id, step_index)
);

-- 13. replay_snapshots
CREATE TABLE IF NOT EXISTS replay_snapshots (
    replay_id TEXT NOT NULL,
    step_index BIGINT NOT NULL,
    round_id TEXT NOT NULL,
    snapshot_kind TEXT NOT NULL,
    allocation_snapshot JSONB,
    settlement_snapshot JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (replay_id, step_index)
);

-- 14. parse_overrides
CREATE TABLE IF NOT EXISTS parse_overrides (
    override_id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL REFERENCES rounds(round_id),
    raw_message_id TEXT NOT NULL,
    corrected_parsed_message JSONB NOT NULL,
    admin_user_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

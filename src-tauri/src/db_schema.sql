-- VaultOne SQLite Local Store schema (ADR-0002 / 0004 / 0005 / 0009).
-- Idempotent migration: CREATE ... IF NOT EXISTS. Run on every open.
-- Naming is snake_case end-to-end (Rust ↔ SQLite ↔ JSONL ↔ TS).

-- Per-request usage detail (ADR-0003: per API request). uuid = dedup key.
CREATE TABLE IF NOT EXISTS usage_records (
    uuid                   TEXT PRIMARY KEY,
    timestamp              TEXT NOT NULL,            -- ISO8601 UTC
    day                    TEXT NOT NULL,            -- yyyy-mm-dd (UTC) bucket
    model                  TEXT NOT NULL,            -- billed / mapped model
    pricing_model          TEXT NOT NULL,            -- key used for price lookup (rebill)
    source                 TEXT NOT NULL,            -- provider tag, e.g. claude_code
    device_id              TEXT NOT NULL,            -- 12-hex owner (ADR-0002)
    input_tokens           INTEGER NOT NULL,
    output_tokens          INTEGER NOT NULL,
    cache_creation_tokens  INTEGER NOT NULL,
    cache_read_tokens      INTEGER NOT NULL,
    server_tool_use        TEXT NOT NULL DEFAULT '{}', -- JSON {web_search,web_fetch}
    input_cost_usd         TEXT NOT NULL,            -- Decimal as TEXT (ADR-0009)
    output_cost_usd        TEXT NOT NULL,
    cache_read_cost_usd    TEXT NOT NULL,
    cache_creation_cost_usd TEXT NOT NULL,
    total_cost_usd         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_usage_day     ON usage_records(day);
CREATE INDEX IF NOT EXISTS idx_usage_model   ON usage_records(model);
CREATE INDEX IF NOT EXISTS idx_usage_device  ON usage_records(device_id);
CREATE INDEX IF NOT EXISTS idx_usage_source  ON usage_records(source);
CREATE INDEX IF NOT EXISTS idx_usage_ts      ON usage_records(timestamp);

-- Dedup ledger (ADR-0005): canonical "have we imported this uuid" set + provenance.
CREATE TABLE IF NOT EXISTS ledger (
    uuid        TEXT PRIMARY KEY,
    source      TEXT NOT NULL,
    device_id   TEXT NOT NULL,
    ingested_at TEXT NOT NULL
);

-- Daily rollups cache (ADR-0009: derived, holds total_cost_usd). Per (day,model,device).
CREATE TABLE IF NOT EXISTS daily_rollups (
    day                    TEXT NOT NULL,
    model                  TEXT NOT NULL,
    device_id              TEXT NOT NULL,
    input_tokens           INTEGER NOT NULL,
    output_tokens          INTEGER NOT NULL,
    cache_creation_tokens  INTEGER NOT NULL,
    cache_read_tokens      INTEGER NOT NULL,
    request_count          INTEGER NOT NULL,
    total_cost_usd         TEXT NOT NULL,
    PRIMARY KEY (day, model, device_id)
);

-- Pricing table (ADR-0006: LiteLLM seed + user overrides). Decimal as TEXT.
CREATE TABLE IF NOT EXISTS model_pricing (
    model_key                TEXT PRIMARY KEY,        -- normalized id
    display_name             TEXT NOT NULL,
    input_per_million        TEXT NOT NULL,           -- USD / 1M tokens
    output_per_million       TEXT NOT NULL,
    cache_read_per_million   TEXT NOT NULL,
    cache_creation_per_million TEXT NOT NULL,
    is_builtin               INTEGER NOT NULL DEFAULT 1, -- 1 = seed, 0 = user
    updated_at               TEXT NOT NULL
);

-- Device registry (ADR-0002).
CREATE TABLE IF NOT EXISTS device (
    device_id   TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    is_self     INTEGER NOT NULL DEFAULT 0,
    first_seen  TEXT NOT NULL
);

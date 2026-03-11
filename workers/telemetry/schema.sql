-- D1 schema for berth-telemetry
-- Run: wrangler d1 execute berth-telemetry --file=schema.sql

CREATE TABLE IF NOT EXISTS telemetry_events (
    id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    app_version TEXT NOT NULL DEFAULT '',
    os_version TEXT NOT NULL DEFAULT '',
    context TEXT DEFAULT '{}',
    occurred_at TEXT NOT NULL,
    ingested_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_device_id ON telemetry_events(device_id);
CREATE INDEX IF NOT EXISTS idx_event_type ON telemetry_events(event_type);
CREATE INDEX IF NOT EXISTS idx_occurred_at ON telemetry_events(occurred_at);
CREATE INDEX IF NOT EXISTS idx_ingested_at ON telemetry_events(ingested_at);

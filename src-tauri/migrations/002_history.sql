CREATE TABLE IF NOT EXISTS query_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id TEXT NOT NULL,
    database_name TEXT NOT NULL,
    collection_name TEXT,
    query_text TEXT NOT NULL,
    query_type TEXT NOT NULL DEFAULT 'shell',
    execution_time_ms INTEGER,
    result_count INTEGER,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_history_connection ON query_history(connection_id);
CREATE INDEX IF NOT EXISTS idx_history_created ON query_history(created_at DESC);

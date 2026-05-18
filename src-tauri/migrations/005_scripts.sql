-- 用户保存的脚本 (Saved Scripts)
CREATE TABLE IF NOT EXISTS scripts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    -- 树形目录路径, 空串表示根目录, 用 "/" 分层, 例如: "线上测试" / "线上测试/查询"
    folder_path TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    -- 可选的默认连接绑定, 打开脚本时如果连接存在就用这个; 不存在时由用户挑选
    connection_id TEXT,
    database_name TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_scripts_folder ON scripts(folder_path);
CREATE INDEX IF NOT EXISTS idx_scripts_name ON scripts(name);

-- 显式目录表, 支持空文件夹 (没有 script 时也保留)
CREATE TABLE IF NOT EXISTS script_folders (
    path TEXT PRIMARY KEY,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

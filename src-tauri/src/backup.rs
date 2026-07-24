//! 备份 / 恢复: **mongodump 兼容**的 BSON 目录格式.
//!
//! 产物布局与 `mongodump --out <dir>` 完全一致, 因此备份可以直接用官方
//! `mongorestore --dir <dir>` 还原, 也可以用本模块的 [`run_restore`] 还原:
//!
//! ```text
//! <target_dir>/<database>/<collection>.bson            # 原始 BSON 文档依次拼接
//! <target_dir>/<database>/<collection>.metadata.json   # options + indexes + uuid + type
//! ```
//!
//! 开启 gzip 时文件名追加 `.gz` (同 `mongodump --gzip`).
//!
//! 与 `query::exporter` 的区别: exporter 面向"把查询结果导成 csv/xlsx 给人看",
//! 有字段投影和类型转换; 本模块面向"整库/整集合原样存档再灌回去", 不做任何
//! 类型转换, 且带上索引定义.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use futures::StreamExt;
use mongodb::bson::{doc, Bson, Document};
use mongodb::{Client, Database};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::AppError;

/// 恢复时每批 insert 的文档数
const RESTORE_BATCH: usize = 500;
/// 单条 BSON 文档长度上限 (MongoDB 硬上限 16MB, 这里放宽到 64MB 兜底防止读到损坏文件时狂吃内存)
const MAX_DOC_BYTES: usize = 64 * 1024 * 1024;
/// 每处理多少条文档发一次进度事件
const PROGRESS_EVERY: u64 = 500;

fn io_err(ctx: &str, e: impl std::fmt::Display) -> AppError {
    AppError::InvalidInput(format!("{ctx}: {e}"))
}

// =====================================================================
//  文件名转义 (与 mongodump 一致的 %XX 方案)
// =====================================================================

/// 集合名 -> 文件名. 集合名里合法但文件名里非法的字符按 `%XX` 转义.
/// `.` 保留不转义 (`system.js` 这类集合 mongodump 也是原样落盘).
fn escape_coll_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '%' => {
                out.push_str(&format!("%{:02X}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

/// 文件名 -> 集合名, [`escape_coll_name`] 的逆操作.
fn unescape_coll_name(file_stem: &str) -> String {
    let bytes = file_stem.as_bytes();
    let mut out = String::with_capacity(file_stem.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&file_stem[i + 1..i + 3], 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        // 非 %XX: 按 UTF-8 字符推进, 避免切断多字节字符
        let ch = file_stem[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn gz_suffix(gzip: bool) -> &'static str {
    if gzip {
        ".gz"
    } else {
        ""
    }
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

// =====================================================================
//  BSON 文件读写
// =====================================================================

/// `.bson` 输出目标: 明文或 gzip. 二者都要显式 finish 才能保证落盘完整
/// (gzip 还要写 trailer), 所以不依赖 Drop.
enum BsonSink {
    Plain(BufWriter<std::fs::File>),
    Gz(GzEncoder<BufWriter<std::fs::File>>),
}

impl Write for BsonSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            BsonSink::Plain(w) => w.write(buf),
            BsonSink::Gz(w) => w.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            BsonSink::Plain(w) => w.flush(),
            BsonSink::Gz(w) => w.flush(),
        }
    }
}

impl BsonSink {
    fn create(path: &Path, gzip: bool) -> Result<Self, AppError> {
        let file = std::fs::File::create(path).map_err(|e| io_err("创建文件失败", e))?;
        let buf = BufWriter::new(file);
        Ok(if gzip {
            BsonSink::Gz(GzEncoder::new(buf, Compression::default()))
        } else {
            BsonSink::Plain(buf)
        })
    }

    fn finish(self) -> Result<(), AppError> {
        match self {
            BsonSink::Plain(mut w) => w.flush().map_err(|e| io_err("写入文件失败", e)),
            BsonSink::Gz(w) => {
                let mut inner = w.finish().map_err(|e| io_err("gzip 压缩失败", e))?;
                inner.flush().map_err(|e| io_err("写入文件失败", e))
            }
        }
    }
}

/// `Send` 是必须的: 恢复时 reader 会跨 `.await` 存活, tauri 要求命令 future 是 Send
fn open_reader(path: &Path, gzip: bool) -> Result<Box<dyn Read + Send>, AppError> {
    let file = std::fs::File::open(path).map_err(|e| io_err("打开文件失败", e))?;
    let buf = BufReader::new(file);
    Ok(if gzip {
        Box::new(GzDecoder::new(buf))
    } else {
        Box::new(buf)
    })
}

/// 同 [`open_reader`], 另外返回一个"已从文件读走多少字节"的计数器.
/// 计数器插在解压**之前**, 所以不论是否 gzip, `已读 / 文件大小` 都是可用的进度比例
/// (BufReader 预读会让它略微超前, 对进度条无所谓).
/// 用 `Arc<AtomicU64>` 而非 `Rc<Cell>`: reader 要跨 await 活着, future 必须是 Send.
fn open_counting_reader(
    path: &Path,
    gzip: bool,
) -> Result<(Box<dyn Read + Send>, std::sync::Arc<std::sync::atomic::AtomicU64>), AppError> {
    let file = std::fs::File::open(path).map_err(|e| io_err("打开文件失败", e))?;
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counting = AtomicCountingReader {
        inner: file,
        read: counter.clone(),
    };
    let buf = BufReader::new(counting);
    let reader: Box<dyn Read + Send> = if gzip {
        Box::new(GzDecoder::new(buf))
    } else {
        Box::new(buf)
    };
    Ok((reader, counter))
}

struct AtomicCountingReader<R> {
    inner: R,
    read: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl<R: Read> Read for AtomicCountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read
            .fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(n)
    }
}

fn write_small_file(path: &Path, data: &[u8], gzip: bool) -> Result<(), AppError> {
    let mut sink = BsonSink::create(path, gzip)?;
    sink.write_all(data).map_err(|e| io_err("写入文件失败", e))?;
    sink.finish()
}

fn read_small_file(path: &Path, gzip: bool) -> Result<String, AppError> {
    let mut r = open_reader(path, gzip)?;
    let mut s = String::new();
    r.read_to_string(&mut s)
        .map_err(|e| io_err("读取文件失败", e))?;
    Ok(s)
}

/// 从 reader 读下一条 BSON 文档. `Ok(None)` 表示正常读到文件末尾.
fn read_next_doc(r: &mut dyn Read) -> Result<Option<Document>, AppError> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(io_err("读取 BSON 失败", e)),
    }
    let len = i32::from_le_bytes(len_buf);
    if len < 5 || len as usize > MAX_DOC_BYTES {
        return Err(AppError::InvalidInput(format!(
            "BSON 文件已损坏: 非法的文档长度 {len}"
        )));
    }
    let mut buf = vec![0u8; len as usize];
    buf[..4].copy_from_slice(&len_buf);
    r.read_exact(&mut buf[4..])
        .map_err(|e| io_err("BSON 文件已损坏 (文档被截断)", e))?;
    Document::from_reader(&buf[..])
        .map(Some)
        .map_err(|e| io_err("解析 BSON 文档失败", e))
}

// =====================================================================
//  服务器命令游标 (listCollections / listIndexes)
// =====================================================================

/// 读完一个 `{cursor: {firstBatch, id, ns}}` 形式的命令结果, 必要时继续 getMore.
async fn read_command_cursor(db: &Database, first: Document) -> Result<Vec<Document>, AppError> {
    let cur = first
        .get_document("cursor")
        .map_err(|_| AppError::InvalidInput("命令返回缺少 cursor 字段".into()))?;
    // ns 形如 "<db>.<coll>" 或 "<db>.$cmd.listCollections"; getMore 要的是第一个 . 之后的部分
    let cursor_coll = cur
        .get_str("ns")
        .ok()
        .and_then(|ns| ns.split_once('.').map(|(_, c)| c.to_string()))
        .unwrap_or_default();
    let mut out: Vec<Document> = cur
        .get_array("firstBatch")
        .map(|a| a.iter().filter_map(|v| v.as_document().cloned()).collect())
        .unwrap_or_default();
    let mut id = cur.get_i64("id").unwrap_or(0);

    while id != 0 && !cursor_coll.is_empty() {
        let res = db
            .run_command(doc! { "getMore": id, "collection": &cursor_coll, "batchSize": 500 })
            .await
            .map_err(AppError::Mongo)?;
        let c = res
            .get_document("cursor")
            .map_err(|_| AppError::InvalidInput("getMore 返回缺少 cursor 字段".into()))?;
        if let Ok(batch) = c.get_array("nextBatch") {
            out.extend(batch.iter().filter_map(|v| v.as_document().cloned()));
        }
        id = c.get_i64("id").unwrap_or(0);
    }
    Ok(out)
}

/// listCollections 的原始结果 (含 name / type / options / info.uuid), 过滤系统集合.
/// `only` 非空时只保留其中列出的集合.
async fn list_collection_specs(db: &Database, only: &[String]) -> Result<Vec<Document>, AppError> {
    let res = db
        .run_command(doc! { "listCollections": 1, "cursor": { "batchSize": 500 } })
        .await
        .map_err(AppError::Mongo)?;
    let mut specs = read_command_cursor(db, res).await?;
    specs.retain(|s| {
        let name = s.get_str("name").unwrap_or("");
        !name.is_empty()
            && !name.starts_with("system.")
            && (only.is_empty() || only.iter().any(|x| x == name))
    });
    specs.sort_by(|a, b| a.get_str("name").unwrap_or("").cmp(b.get_str("name").unwrap_or("")));
    Ok(specs)
}

/// listIndexes 的原始索引定义 (含 v / key / name / 各类 option).
async fn list_index_specs(db: &Database, coll: &str) -> Result<Vec<Document>, AppError> {
    let res = db
        .run_command(doc! { "listIndexes": coll, "cursor": { "batchSize": 500 } })
        .await
        .map_err(AppError::Mongo)?;
    read_command_cursor(db, res).await
}

// =====================================================================
//  备份
// =====================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRequest {
    pub connection_id: String,
    pub database: String,
    /// 要备份的集合; 空数组 = 整库
    #[serde(default)]
    pub collections: Vec<String>,
    /// 备份根目录; 实际写入 `<target_dir>/<database>/`
    pub target_dir: String,
    /// 是否 gzip 压缩 (等价 `mongodump --gzip`)
    #[serde(default)]
    pub gzip: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    /// 最终落盘的库目录 `<target_dir>/<database>`
    pub output_dir: String,
    pub collections: usize,
    pub documents: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupProgress {
    phase: String,
    collection: String,
    /// 当前是第几个集合 (1-based)
    coll_index: usize,
    coll_total: usize,
    /// 当前集合已处理文档数
    docs_done: u64,
    /// 当前集合预估总数, -1 表示未知
    docs_total: i64,
    /// 全部集合累计已处理文档数
    total_done: u64,
}

#[allow(clippy::too_many_arguments)]
fn emit_backup(
    app: &AppHandle,
    phase: &str,
    collection: &str,
    coll_index: usize,
    coll_total: usize,
    docs_done: u64,
    docs_total: i64,
    total_done: u64,
) {
    let _ = app.emit(
        "backup-progress",
        BackupProgress {
            phase: phase.into(),
            collection: collection.into(),
            coll_index,
            coll_total,
            docs_done,
            docs_total,
            total_done,
        },
    );
}

/// 组装 mongodump 的 `<coll>.metadata.json` 内容.
/// 字段顺序与 mongodump 保持一致: options / indexes / uuid / collectionName / type.
fn build_metadata(spec: &Document, name: &str, coll_type: &str, indexes: Vec<Document>) -> Document {
    let options = spec.get_document("options").cloned().unwrap_or_default();
    let mut meta = doc! {
        "options": options,
        "indexes": Bson::Array(indexes.into_iter().map(Bson::Document).collect()),
    };
    // uuid 是 Binary(subtype 4), mongodump 写成无连字符 hex 字符串
    if let Some(Bson::Binary(bin)) = spec.get_document("info").ok().and_then(|i| i.get("uuid")) {
        meta.insert("uuid", to_hex(&bin.bytes));
    }
    meta.insert("collectionName", name);
    meta.insert("type", coll_type);
    meta
}

pub async fn run_backup(
    client: &Client,
    app: &AppHandle,
    req: &BackupRequest,
) -> Result<BackupSummary, AppError> {
    if req.target_dir.trim().is_empty() {
        return Err(AppError::InvalidInput("请选择备份目录".into()));
    }
    let db = client.database(&req.database);
    let out_dir = PathBuf::from(req.target_dir.trim()).join(&req.database);
    std::fs::create_dir_all(&out_dir).map_err(|e| io_err("创建备份目录失败", e))?;

    emit_backup(app, "读取集合列表...", "", 0, 0, 0, -1, 0);
    let specs = list_collection_specs(&db, &req.collections).await?;
    if specs.is_empty() {
        return Err(AppError::InvalidInput("没有可备份的集合".into()));
    }

    let coll_total = specs.len();
    let mut total_docs: u64 = 0;
    let mut total_bytes: u64 = 0;

    for (i, spec) in specs.iter().enumerate() {
        let name = spec.get_str("name").unwrap_or_default().to_string();
        let coll_type = spec.get_str("type").unwrap_or("collection").to_string();
        let stem = escape_coll_name(&name);
        let idx = i + 1;

        emit_backup(app, "备份中", &name, idx, coll_total, 0, -1, total_docs);

        // 1) metadata.json (视图没有索引, listIndexes 会报错, 失败当空索引处理)
        let indexes = list_index_specs(&db, &name).await.unwrap_or_default();
        let meta = build_metadata(spec, &name, &coll_type, indexes);
        let meta_json = serde_json::to_string(&meta).map_err(|e| io_err("序列化 metadata 失败", e))?;
        let meta_path = out_dir.join(format!("{stem}.metadata.json{}", gz_suffix(req.gzip)));
        write_small_file(&meta_path, meta_json.as_bytes(), req.gzip)?;
        total_bytes += file_len(&meta_path);

        // 2) .bson —— 视图没有自己的数据, 但仍写一个空文件, 这样 mongorestore
        //    才会为它建 intent 并按 metadata 重建视图.
        let bson_path = out_dir.join(format!("{stem}.bson{}", gz_suffix(req.gzip)));
        if coll_type == "view" {
            write_small_file(&bson_path, &[], req.gzip)?;
        } else {
            let n = dump_collection_docs(
                &db, &name, &bson_path, req.gzip, app, idx, coll_total, total_docs,
            )
            .await?;
            total_docs += n;
        }
        total_bytes += file_len(&bson_path);
    }

    emit_backup(app, "完成", "", coll_total, coll_total, 0, -1, total_docs);
    Ok(BackupSummary {
        output_dir: out_dir.to_string_lossy().to_string(),
        collections: coll_total,
        documents: total_docs,
        bytes: total_bytes,
    })
}

#[allow(clippy::too_many_arguments)]
async fn dump_collection_docs(
    db: &Database,
    coll_name: &str,
    path: &Path,
    gzip: bool,
    app: &AppHandle,
    coll_index: usize,
    coll_total: usize,
    total_before: u64,
) -> Result<u64, AppError> {
    let coll = db.collection::<Document>(coll_name);
    // 进度条用的估算值, 走 count 元数据, 不扫全表
    let est = coll.estimated_document_count().await.unwrap_or(0) as i64;

    let mut sink = BsonSink::create(path, gzip)?;
    let mut cursor = coll
        .find(doc! {})
        .batch_size(1000)
        .await
        .map_err(AppError::Mongo)?;

    let mut n: u64 = 0;
    while let Some(item) = cursor.next().await {
        let d = item.map_err(AppError::Mongo)?;
        d.to_writer(&mut sink)
            .map_err(|e| io_err("写入 BSON 失败", e))?;
        n += 1;
        if n % PROGRESS_EVERY == 0 {
            emit_backup(
                app,
                "备份中",
                coll_name,
                coll_index,
                coll_total,
                n,
                est,
                total_before + n,
            );
        }
    }
    sink.finish()?;
    emit_backup(
        app,
        "备份中",
        coll_name,
        coll_index,
        coll_total,
        n,
        n as i64,
        total_before + n,
    );
    Ok(n)
}

// =====================================================================
//  扫描备份目录
// =====================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCollInfo {
    pub name: String,
    pub file_name: String,
    pub size: u64,
    pub gzip: bool,
    pub has_metadata: bool,
    /// collection | view | timeseries
    pub coll_type: String,
    pub index_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupDirInfo {
    /// 从目录名推断出的原始库名
    pub database: String,
    /// 真正含 .bson 的库目录 (用户可能选了它的上级备份根目录)
    pub dir: String,
    pub collections: Vec<BackupCollInfo>,
}

fn dir_has_bson(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        name.ends_with(".bson") || name.ends_with(".bson.gz")
    })
}

/// 读 `<coll>.metadata.json`, 内容是 Extended JSON v2.
fn read_metadata(path: &Path, gzip: bool) -> Option<Document> {
    let text = read_small_file(path, gzip).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    match Bson::try_from(value).ok()? {
        Bson::Document(d) => Some(d),
        _ => None,
    }
}

/// 扫描用户选中的目录, 返回可恢复的集合清单.
/// 允许用户选备份根目录 (里面只有一个库目录时自动下钻)。
pub fn scan_backup_dir(path: &str) -> Result<BackupDirInfo, AppError> {
    let root = PathBuf::from(path.trim());
    if !root.is_dir() {
        return Err(AppError::InvalidInput("所选路径不是一个目录".into()));
    }

    let db_dir = if dir_has_bson(&root) {
        root.clone()
    } else {
        let mut candidates: Vec<PathBuf> = std::fs::read_dir(&root)
            .map_err(|e| io_err("读取目录失败", e))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && dir_has_bson(p))
            .collect();
        candidates.sort();
        match candidates.len() {
            1 => candidates.remove(0),
            0 => {
                return Err(AppError::InvalidInput(
                    "所选目录下没有找到 .bson 备份文件".into(),
                ))
            }
            n => {
                return Err(AppError::InvalidInput(format!(
                    "所选目录下有 {n} 个数据库的备份, 请具体选中其中某一个数据库目录"
                )))
            }
        }
    };

    let mut collections = Vec::new();
    for entry in std::fs::read_dir(&db_dir)
        .map_err(|e| io_err("读取目录失败", e))?
        .flatten()
    {
        let file_path = entry.path();
        if !file_path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let (stem, gzip) = if let Some(s) = file_name.strip_suffix(".bson.gz") {
            (s, true)
        } else if let Some(s) = file_name.strip_suffix(".bson") {
            (s, false)
        } else {
            continue;
        };

        let meta_path = db_dir.join(format!("{stem}.metadata.json{}", gz_suffix(gzip)));
        let meta = read_metadata(&meta_path, gzip);
        collections.push(BackupCollInfo {
            name: unescape_coll_name(stem),
            file_name: file_name.clone(),
            size: file_len(&file_path),
            gzip,
            has_metadata: meta.is_some(),
            coll_type: meta
                .as_ref()
                .and_then(|m| m.get_str("type").ok())
                .unwrap_or("collection")
                .to_string(),
            index_count: meta
                .as_ref()
                .and_then(|m| m.get_array("indexes").ok())
                .map(|a| a.len())
                .unwrap_or(0),
        });
    }
    collections.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(BackupDirInfo {
        database: db_dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        dir: db_dir.to_string_lossy().to_string(),
        collections,
    })
}

// =====================================================================
//  恢复
// =====================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreRequest {
    pub connection_id: String,
    /// 含 .bson 的库目录 (由 scan_backup_dir 返回的 `dir`)
    pub source_dir: String,
    pub target_database: String,
    /// 要恢复的集合; 空数组 = 全部
    #[serde(default)]
    pub collections: Vec<String>,
    /// drop = 先删目标集合再灌; insert = 直接插 (_id 冲突报错);
    /// skip = 跳过 _id 冲突; overwrite = 按 _id upsert
    pub mode: String,
    /// 是否按 metadata 重建索引
    #[serde(default)]
    pub restore_indexes: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSummary {
    pub collections: usize,
    pub documents: u64,
    pub indexes: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RestoreProgress {
    phase: String,
    collection: String,
    coll_index: usize,
    coll_total: usize,
    docs_done: u64,
    total_done: u64,
    /// 当前集合已读取的文件字节数 / 文件总字节数 —— 单集合恢复时进度条靠这个动
    bytes_done: u64,
    bytes_total: u64,
}

#[allow(clippy::too_many_arguments)]
fn emit_restore(
    app: &AppHandle,
    phase: &str,
    collection: &str,
    coll_index: usize,
    coll_total: usize,
    docs_done: u64,
    total_done: u64,
    bytes_done: u64,
    bytes_total: u64,
) {
    let _ = app.emit(
        "restore-progress",
        RestoreProgress {
            phase: phase.into(),
            collection: collection.into(),
            coll_index,
            coll_total,
            docs_done,
            total_done,
            bytes_done,
            bytes_total,
        },
    );
}

pub async fn run_restore(
    client: &Client,
    app: &AppHandle,
    req: &RestoreRequest,
) -> Result<RestoreSummary, AppError> {
    if req.target_database.trim().is_empty() {
        return Err(AppError::InvalidInput("请选择目标数据库".into()));
    }
    let info = scan_backup_dir(&req.source_dir)?;
    let selected: Vec<&BackupCollInfo> = info
        .collections
        .iter()
        .filter(|c| req.collections.is_empty() || req.collections.iter().any(|n| n == &c.name))
        .collect();
    if selected.is_empty() {
        return Err(AppError::InvalidInput("没有选中任何要恢复的集合".into()));
    }

    let db = client.database(req.target_database.trim());
    let db_dir = PathBuf::from(&info.dir);
    let coll_total = selected.len();
    let mut total_docs: u64 = 0;
    let mut total_indexes = 0usize;
    let mut warnings: Vec<String> = Vec::new();

    for (i, item) in selected.iter().enumerate() {
        let idx = i + 1;
        let name = &item.name;
        let stem = escape_coll_name(name);
        emit_restore(app, "恢复中", name, idx, coll_total, 0, total_docs, 0, item.size);

        let meta = read_metadata(
            &db_dir.join(format!("{stem}.metadata.json{}", gz_suffix(item.gzip))),
            item.gzip,
        );

        // 1) drop 模式: 先删掉目标集合, 再按 metadata 里的 options 重建
        //    (capped / timeseries / view 这类必须建集合时就带上 options)
        if req.mode == "drop" {
            db.collection::<Document>(name)
                .drop()
                .await
                .map_err(AppError::Mongo)?;
        }
        let options = meta
            .as_ref()
            .and_then(|m| m.get_document("options").ok().cloned())
            .unwrap_or_default();
        if req.mode == "drop" || item.coll_type == "view" {
            let mut cmd = doc! { "create": name.as_str() };
            for (k, v) in options.iter() {
                cmd.insert(k.clone(), v.clone());
            }
            if let Err(e) = db.run_command(cmd).await {
                // 集合已存在等情况不致命, 继续往下灌数据
                if item.coll_type == "view" {
                    warnings.push(format!("视图 {name} 创建失败: {e}"));
                    continue;
                }
            }
        }

        // 2) 灌数据 (视图没有数据)
        if item.coll_type != "view" {
            let n = restore_collection_docs(
                &db,
                name,
                &db_dir.join(&item.file_name),
                item.gzip,
                &req.mode,
                app,
                idx,
                coll_total,
                total_docs,
                item.size,
            )
            .await?;
            total_docs += n;
        }

        // 3) 重建索引
        if req.restore_indexes && item.coll_type != "view" {
            match restore_indexes(&db, name, meta.as_ref()).await {
                Ok(n) => total_indexes += n,
                Err(e) => warnings.push(format!("{name} 索引重建失败: {e}")),
            }
        }
    }

    emit_restore(app, "完成", "", coll_total, coll_total, 0, total_docs, 0, 0);
    Ok(RestoreSummary {
        collections: coll_total,
        documents: total_docs,
        indexes: total_indexes,
        warnings,
    })
}

#[allow(clippy::too_many_arguments)]
async fn restore_collection_docs(
    db: &Database,
    coll_name: &str,
    path: &Path,
    gzip: bool,
    mode: &str,
    app: &AppHandle,
    coll_index: usize,
    coll_total: usize,
    total_before: u64,
    file_size: u64,
) -> Result<u64, AppError> {
    let coll = db.collection::<Document>(coll_name);
    let (mut reader, bytes_read) = open_counting_reader(path, gzip)?;
    let mut batch: Vec<Document> = Vec::with_capacity(RESTORE_BATCH);
    let mut n: u64 = 0;

    loop {
        let next = read_next_doc(reader.as_mut())?;
        let eof = next.is_none();
        if let Some(d) = next {
            batch.push(d);
        }
        if batch.len() >= RESTORE_BATCH || (eof && !batch.is_empty()) {
            n += insert_batch(&coll, &batch, mode).await?;
            batch.clear();
            emit_restore(
                app,
                "恢复中",
                coll_name,
                coll_index,
                coll_total,
                n,
                total_before + n,
                bytes_read
                    .load(std::sync::atomic::Ordering::Relaxed)
                    .min(file_size),
                file_size,
            );
        }
        if eof {
            break;
        }
    }
    Ok(n)
}

async fn insert_batch(
    coll: &mongodb::Collection<Document>,
    batch: &[Document],
    mode: &str,
) -> Result<u64, AppError> {
    match mode {
        // 按 _id upsert: 有则整条替换, 无则插入
        "overwrite" => {
            let mut n = 0u64;
            for d in batch {
                match d.get("_id") {
                    Some(id) => {
                        coll.replace_one(doc! { "_id": id.clone() }, d.clone())
                            .upsert(true)
                            .await
                            .map_err(AppError::Mongo)?;
                    }
                    None => {
                        coll.insert_one(d.clone()).await.map_err(AppError::Mongo)?;
                    }
                }
                n += 1;
            }
            Ok(n)
        }
        // ordered:false, _id 重复的那几条失败, 其余照插.
        // 部分失败时驱动只给一个聚合错误, 拿不到精确插入数, 按整批计 (与 stream_import 一致)
        "skip" => match coll.insert_many(batch.to_vec()).ordered(false).await {
            Ok(r) => Ok(r.inserted_ids.len() as u64),
            Err(_) => Ok(batch.len() as u64),
        },
        // "drop" / "insert": 目标是空集合或用户接受冲突报错
        _ => {
            coll.insert_many(batch.to_vec())
                .await
                .map_err(AppError::Mongo)?;
            Ok(batch.len() as u64)
        }
    }
}

/// 按 metadata 里的原始索引定义重建索引. 跳过默认的 `_id_`.
/// 直接用 createIndexes 命令下发原始定义, 避免经 IndexModel 中转丢失冷门选项.
async fn restore_indexes(
    db: &Database,
    coll_name: &str,
    meta: Option<&Document>,
) -> Result<usize, AppError> {
    let Some(indexes) = meta.and_then(|m| m.get_array("indexes").ok()) else {
        return Ok(0);
    };
    let specs: Vec<Bson> = indexes
        .iter()
        .filter_map(|v| v.as_document())
        .filter(|d| d.get_str("name").unwrap_or("") != "_id_")
        .map(|d| {
            let mut spec = d.clone();
            // ns 在 4.4+ 已不是合法的 createIndexes 参数
            spec.remove("ns");
            Bson::Document(spec)
        })
        .collect();
    if specs.is_empty() {
        return Ok(0);
    }
    let count = specs.len();
    db.run_command(doc! { "createIndexes": coll_name, "indexes": specs })
        .await
        .map_err(AppError::Mongo)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::DateTime;

    #[test]
    fn escapes_and_unescapes_collection_names() {
        for name in ["users", "system.js", "a.b.c", "中文集合", "wei|rd", "a/b"] {
            let stem = escape_coll_name(name);
            assert!(!stem.contains('/') && !stem.contains('|'), "{stem} 仍含非法字符");
            assert_eq!(unescape_coll_name(&stem), name);
        }
    }

    #[test]
    fn bson_stream_roundtrips_through_sink() {
        let dir = std::env::temp_dir().join("mongopilot_backup_test");
        std::fs::create_dir_all(&dir).unwrap();

        for gzip in [false, true] {
            let path = dir.join(if gzip { "t.bson.gz" } else { "t.bson" });
            let docs = vec![
                doc! { "_id": 1i32, "t": DateTime::from_millis(1_769_392_333_000) },
                doc! { "_id": 2i32, "s": "hello", "nested": { "a": [1i32, 2i32] } },
            ];

            let mut sink = BsonSink::create(&path, gzip).unwrap();
            for d in &docs {
                d.to_writer(&mut sink).unwrap();
            }
            sink.finish().unwrap();

            let mut reader = open_reader(&path, gzip).unwrap();
            let mut back = Vec::new();
            while let Some(d) = read_next_doc(reader.as_mut()).unwrap() {
                back.push(d);
            }
            assert_eq!(back, docs, "gzip={gzip} 时 BSON 往返不一致");
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn metadata_roundtrips_through_extended_json() {
        let spec = doc! {
            "name": "users",
            "type": "collection",
            "options": { "capped": true, "size": 1024i64 },
        };
        let indexes = vec![
            doc! { "v": 2i32, "key": { "_id": 1i32 }, "name": "_id_" },
            doc! { "v": 2i32, "key": { "email": 1i32 }, "name": "email_1", "unique": true },
        ];
        let meta = build_metadata(&spec, "users", "collection", indexes);
        let json = serde_json::to_string(&meta).unwrap();

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let back = match Bson::try_from(value).unwrap() {
            Bson::Document(d) => d,
            other => panic!("应还原成 Document, 实际 {other:?}"),
        };
        assert_eq!(back.get_str("collectionName").unwrap(), "users");
        assert_eq!(back.get_str("type").unwrap(), "collection");
        assert!(back.get_document("options").unwrap().get_bool("capped").unwrap());
        let idx = back.get_array("indexes").unwrap();
        assert_eq!(idx.len(), 2);
        let second = idx[1].as_document().unwrap();
        assert_eq!(second.get_str("name").unwrap(), "email_1");
        // key 的 1 必须还原成数字而不是字符串, 否则 createIndexes 会拒绝
        assert!(second.get_document("key").unwrap().get_i32("email").is_ok());
    }

    #[test]
    fn rejects_corrupt_bson_length() {
        // 长度字段声称 3 字节 (< 最小的 5), 应报错而不是 panic
        let bytes: &[u8] = &[3, 0, 0, 0];
        let mut r = bytes;
        assert!(read_next_doc(&mut r).is_err());
    }

    #[test]
    fn empty_file_reads_as_no_documents() {
        let bytes: &[u8] = &[];
        let mut r = bytes;
        assert!(read_next_doc(&mut r).unwrap().is_none());
    }
}

import { invoke } from "./invoke";

// =====================================================================
//  备份 / 恢复 —— mongodump 兼容的 BSON 目录格式
//
//  产物: <targetDir>/<database>/<collection>.bson (+ .metadata.json)
//  开 gzip 时文件名追加 .gz, 与 `mongodump --gzip` 一致, 可直接用官方
//  `mongorestore --dir <targetDir>` 还原.
// =====================================================================

export interface BackupRequest {
  connectionId: string;
  database: string;
  /** 要备份的集合; 空数组 = 整库 */
  collections: string[];
  /** 备份根目录, 实际写到 <targetDir>/<database>/ */
  targetDir: string;
  gzip: boolean;
}

export interface BackupSummary {
  outputDir: string;
  collections: number;
  documents: number;
  bytes: number;
}

/** `backup-progress` 事件负载 */
export interface BackupProgress {
  phase: string;
  collection: string;
  collIndex: number;
  collTotal: number;
  docsDone: number;
  /** 当前集合预估总数, -1 = 未知 */
  docsTotal: number;
  totalDone: number;
}

export interface BackupCollInfo {
  name: string;
  fileName: string;
  size: number;
  gzip: boolean;
  hasMetadata: boolean;
  /** collection | view | timeseries */
  collType: string;
  indexCount: number;
}

export interface BackupDirInfo {
  /** 从目录名推断的原始库名 */
  database: string;
  /** 真正含 .bson 的库目录 */
  dir: string;
  collections: BackupCollInfo[];
}

export type RestoreMode = "drop" | "insert" | "skip" | "overwrite";

export interface RestoreRequest {
  connectionId: string;
  sourceDir: string;
  targetDatabase: string;
  /** 要恢复的集合; 空数组 = 全部 */
  collections: string[];
  mode: RestoreMode;
  restoreIndexes: boolean;
}

export interface RestoreSummary {
  collections: number;
  documents: number;
  indexes: number;
  warnings: string[];
}

/** `restore-progress` 事件负载 */
export interface RestoreProgress {
  phase: string;
  collection: string;
  collIndex: number;
  collTotal: number;
  docsDone: number;
  totalDone: number;
  /** 当前集合已读文件字节 / 文件总字节, 用来在集合内部插值进度 */
  bytesDone: number;
  bytesTotal: number;
}

export function backupDatabase(request: BackupRequest): Promise<BackupSummary> {
  return invoke<BackupSummary>("backup_database", { request });
}

export function scanBackupDir(path: string): Promise<BackupDirInfo> {
  return invoke<BackupDirInfo>("scan_backup_dir", { path });
}

export function restoreBackup(request: RestoreRequest): Promise<RestoreSummary> {
  return invoke<RestoreSummary>("restore_backup", { request });
}

/** 字节数 -> 人类可读 (1.2 MB) */
export function formatBytes(n: number): string {
  if (!n || n < 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v : v.toFixed(1)} ${units[i]}`;
}

/** 默认备份子目录名: <db>-backup-20260724-153000, 避免重复备份互相覆盖 */
export function defaultBackupFolderName(database: string): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  const stamp =
    `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}` +
    `-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
  return `${database}-backup-${stamp}`;
}

/** 拼路径, 沿用用户输入里的分隔符风格 */
export function joinPath(dir: string, name: string): string {
  const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
  return dir.replace(/[\\/]+$/, "") + sep + name;
}

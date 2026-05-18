export interface ScriptInfo {
  id: string;
  name: string;
  /** 树形目录路径, 空串=根目录, 用 "/" 分层 */
  folderPath: string;
  content: string;
  /** 默认连接绑定 (打开脚本时优先用) */
  connectionId: string | null;
  databaseName: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface ScriptFolderInfo {
  path: string;
  sortOrder: number;
  createdAt: string;
}

export interface SaveScriptInput {
  /** 留空 → 新建; 给值 → 按 id 更新 */
  id?: string;
  name: string;
  folderPath?: string;
  content?: string;
  connectionId?: string | null;
  databaseName?: string | null;
  sortOrder?: number;
}

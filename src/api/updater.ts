import { invoke } from "./invoke";

export interface UpdateInfo {
  currentVersion: string;
  latestVersion: string;
  hasUpdate: boolean;
  releaseUrl: string;
  notes: string;
  publishedAt: string;
  assetUrl: string | null;
  assetName: string | null;
  assetSize: number | null;
}

/** 查 GitHub 最新 release, 对比当前版本. 有网络问题 / 尚无 release 时会 throw. */
export async function checkForUpdates(): Promise<UpdateInfo> {
  return invoke<UpdateInfo>("check_for_updates");
}

/** 下载进度事件 `update-download-progress` 的载荷 */
export interface DownloadProgress {
  downloaded: number;
  /** 0 表示服务端没给 Content-Length */
  total: number;
}

/**
 * 下载安装包到临时目录, 返回本地路径。
 * 过程中后端 emit `update-download-progress`, 调用方自己 listen。
 */
export async function downloadUpdate(url: string, fileName: string): Promise<string> {
  return invoke<string>("download_update", { url, fileName });
}

/**
 * 启动下载好的安装程序。
 * Windows 上安装程序起来后应用会自动退出 (不退出装不上, 文件被占用)。
 */
export async function installUpdate(path: string): Promise<void> {
  return invoke<void>("install_update", { path });
}

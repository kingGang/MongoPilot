import { invoke } from "@tauri-apps/api/core";

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

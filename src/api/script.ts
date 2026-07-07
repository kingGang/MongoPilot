import { invoke } from "./invoke";
import type { ScriptFolderInfo, ScriptInfo, SaveScriptInput } from "@/types/script";

export async function listScripts(): Promise<ScriptInfo[]> {
  return invoke<ScriptInfo[]>("list_scripts");
}

export async function listScriptFolders(): Promise<ScriptFolderInfo[]> {
  return invoke<ScriptFolderInfo[]>("list_script_folders");
}

export async function getScript(id: string): Promise<ScriptInfo> {
  return invoke<ScriptInfo>("get_script", { id });
}

export async function saveScript(input: SaveScriptInput): Promise<ScriptInfo> {
  return invoke<ScriptInfo>("save_script", { input });
}

export async function deleteScript(id: string): Promise<void> {
  return invoke("delete_script", { id });
}

export async function createScriptFolder(path: string): Promise<void> {
  return invoke("create_script_folder", { path });
}

export async function deleteScriptFolder(path: string, cascade = false): Promise<void> {
  return invoke("delete_script_folder", { path, cascade });
}

export async function renameScriptFolder(oldPath: string, newPath: string): Promise<void> {
  return invoke("rename_script_folder", { oldPath, newPath });
}

export interface ImportSummary {
  imported: number;
  skipped: number;
  foldersCreated: number;
}

export async function importScriptFiles(
  paths: string[],
  targetFolder?: string,
): Promise<ImportSummary> {
  return invoke<ImportSummary>("import_script_files", { paths, targetFolder });
}

export async function importScriptDirectory(
  rootPath: string,
  targetFolder?: string,
): Promise<ImportSummary> {
  return invoke<ImportSummary>("import_script_directory", { rootPath, targetFolder });
}

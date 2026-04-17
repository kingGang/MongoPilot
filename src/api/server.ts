import { invoke } from "@tauri-apps/api/core";
import type { ServerStatus, UserInfo, CreateUserRequest, ProfileEntry } from "@/types/server";

export async function getServerStatus(connectionId: string): Promise<ServerStatus> {
  return invoke<ServerStatus>("get_server_status", { connectionId });
}

export async function explainQuery(
  connectionId: string,
  database: string,
  collection: string,
  filter: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  return invoke<Record<string, unknown>>("explain_query", {
    connectionId,
    database,
    collection,
    filter,
  });
}

export async function listUsers(connectionId: string, database: string): Promise<UserInfo[]> {
  return invoke<UserInfo[]>("list_users", { connectionId, database });
}

export async function createUser(
  connectionId: string,
  database: string,
  request: CreateUserRequest,
): Promise<void> {
  return invoke("create_user", { connectionId, database, request });
}

export async function dropUser(
  connectionId: string,
  database: string,
  username: string,
): Promise<void> {
  return invoke("drop_user", { connectionId, database, username });
}

export async function getProfilerStatus(
  connectionId: string,
  database: string,
): Promise<Record<string, unknown>> {
  return invoke<Record<string, unknown>>("get_profiler_status", { connectionId, database });
}

export async function setProfilerLevel(
  connectionId: string,
  database: string,
  level: number,
  slowMs?: number,
): Promise<void> {
  return invoke("set_profiler_level", { connectionId, database, level, slowMs });
}

export async function getProfilerData(
  connectionId: string,
  database: string,
  limit?: number,
): Promise<ProfileEntry[]> {
  return invoke<ProfileEntry[]>("get_profiler_data", { connectionId, database, limit });
}

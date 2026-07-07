import { invoke } from "./invoke";
import type { ConnectionConfig, ServerInfo } from "@/types/connection";

export async function listConnections(): Promise<ConnectionConfig[]> {
  return invoke<ConnectionConfig[]>("list_connections");
}

export async function getConnection(id: string): Promise<ConnectionConfig> {
  return invoke<ConnectionConfig>("get_connection", { id });
}

export async function saveConnection(config: ConnectionConfig): Promise<void> {
  return invoke("save_connection", { config });
}

export async function deleteConnection(id: string): Promise<void> {
  return invoke("delete_connection", { id });
}

export async function testConnection(config: ConnectionConfig): Promise<ServerInfo> {
  return invoke<ServerInfo>("test_connection", { config });
}

export async function connectToServer(config: ConnectionConfig): Promise<void> {
  return invoke("connect", { config });
}

export async function disconnect(id: string): Promise<void> {
  return invoke("disconnect", { id });
}

export async function parseUri(uri: string): Promise<ConnectionConfig> {
  return invoke<ConnectionConfig>("parse_uri", { uri });
}

export async function exportUri(config: ConnectionConfig): Promise<string> {
  return invoke<string>("export_uri", { config });
}

export async function activeConnections(): Promise<string[]> {
  return invoke<string[]>("active_connections");
}

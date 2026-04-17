export interface ServerStatus {
  host: string;
  version: string;
  uptime: number;
  connections: { current: number; available: number; totalCreated: number };
  opcounters: {
    insert: number;
    query: number;
    update: number;
    delete: number;
    getmore: number;
    command: number;
  };
  memory: { resident: number; virtualMem: number; mapped: number };
  storageEngine: string;
}

export interface UserInfo {
  user: string;
  database: string;
  roles: { role: string; db: string }[];
}

export interface CreateUserRequest {
  username: string;
  password: string;
  roles: { role: string; db: string }[];
}

export interface ProfileEntry {
  op: string;
  ns: string;
  millis: number;
  ts: string;
  command: Record<string, unknown>;
  planSummary: string;
}

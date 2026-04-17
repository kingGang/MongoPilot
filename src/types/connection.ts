export interface ConnectionConfig {
  id: string;
  name: string;
  groupPath: string;
  color: string | null;
  connType: "standalone" | "replicaset" | "sharded";
  host: string;
  port: number;
  authType: "none" | "password" | "x509" | "ldap";
  username: string | null;
  password: string | null;
  authDb: string | null;
  replicaSet: string | null;
  srv: boolean;
  tls: boolean;
  tlsCaFile: string | null;
  tlsCertFile: string | null;
  tlsKeyFile: string | null;
  tlsAllowInvalid: boolean;
  sshEnabled: boolean;
  sshHost: string | null;
  sshPort: number | null;
  sshUsername: string | null;
  sshAuthType: "password" | "privateKey" | null;
  sshPassword: string | null;
  sshPrivateKey: string | null;
  sshPassphrase: string | null;
  defaultDb: string | null;
  uriOptions: string | null;
  readOnly: boolean;
  sortOrder: number;
}

export interface ServerInfo {
  version: string;
  topology: string;
  replicaSet: string | null;
}

export function createDefaultConnection(): ConnectionConfig {
  return {
    id: crypto.randomUUID(),
    name: "New Connection",
    groupPath: "",
    color: null,
    connType: "standalone",
    host: "localhost",
    port: 27017,
    authType: "none",
    username: null,
    password: null,
    authDb: "admin",
    replicaSet: null,
    srv: false,
    tls: false,
    tlsCaFile: null,
    tlsCertFile: null,
    tlsKeyFile: null,
    tlsAllowInvalid: false,
    sshEnabled: false,
    sshHost: null,
    sshPort: 22,
    sshUsername: null,
    sshAuthType: "password",
    sshPassword: null,
    sshPrivateKey: null,
    sshPassphrase: null,
    defaultDb: null,
    uriOptions: null,
    readOnly: false,
    sortOrder: 0,
  };
}

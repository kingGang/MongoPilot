export interface CollectionStats {
  documentCount: number;
  totalSize: number;
  avgDocumentSize: number;
  indexCount: number;
  totalIndexSize: number;
}

export interface IndexInfo {
  name: string;
  keys: Record<string, unknown>;
  unique: boolean;
  sparse: boolean;
}

export interface CreateIndexOptions {
  unique?: boolean;
  sparse?: boolean;
  name?: string;
  expireAfterSeconds?: number;
  background?: boolean;
}

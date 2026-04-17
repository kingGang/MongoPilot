export interface AiSettings {
  provider: "claude" | "openai" | "custom";
  apiKey: string;
  model: string;
  baseUrl?: string;
  temperature?: number;
}

export interface ChatMessage {
  role: "user" | "assistant" | "system";
  content: string;
}

export interface SchemaInfo {
  collection: string;
  sampleCount: number;
  fields: FieldInfo[];
}

export interface FieldInfo {
  name: string;
  fieldTypes: { bsonType: string; count: number }[];
  occurrencePercent: number;
  sampleValues: string[];
}

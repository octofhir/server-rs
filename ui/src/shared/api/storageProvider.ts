/** Administrative connection metadata. Stored key values are write-only. */
export type StorageCredentials =
  | { type: "environment"; accessKeyIdEnv: string; secretAccessKeyEnv: string; sessionTokenEnv?: string | null }
  | { type: "stored"; accessKeyId?: string; secretAccessKey?: string; sessionToken?: string };
export interface StorageProvider {
  resourceType: "StorageProvider";
  id: string;
  name?: string | null;
  enabled: boolean;
  type: "s3";
  endpoint?: string | null;
  region: string;
  addressingStyle: "path" | "virtual_hosted";
  allowHttp: boolean;
  credentials: StorageCredentials;
  meta?: { versionId: string; lastUpdated: string };
}
export function isStorageProvider(value: unknown): value is StorageProvider {
  if (!value || typeof value !== "object") return false;
  const item = value as Partial<StorageProvider>;
  return item.resourceType === "StorageProvider" && typeof item.id === "string" &&
    typeof item.enabled === "boolean" && item.type === "s3" && typeof item.region === "string" &&
    (item.addressingStyle === "path" || item.addressingStyle === "virtual_hosted") &&
    (item.credentials?.type === "stored" || item.credentials?.type === "environment") &&
    typeof item.meta?.versionId === "string";
}

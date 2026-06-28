import type { SqlResponse } from "@/shared/api/types";

/** A single executed (or replayed-from-history) query in the console session. */
export interface StreamEntry {
  id: string;
  query: string;
  result?: SqlResponse;
  error?: string;
  explainData?: SqlResponse;
  executionTimeMs?: number;
  timestamp: Date;
  status: "success" | "error" | "pending";
  isExpanded: boolean;
  fromHistory?: boolean;
}

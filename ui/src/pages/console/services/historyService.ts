import { fhirClient } from "../../../shared/api/fhirClient";
import type { HistoryEntry } from "../db/historyDatabase";

const MAX_HISTORY_ENTRIES = 200;
const RESPONSE_SIZE_LIMIT = 256 * 1024; // 256KB
const ANONYMOUS_USER = "anonymous";

/**
 * FHIR ConsoleHistoryEntry resource shape (octofhir-console IG).
 * Stored at root `/ConsoleHistoryEntry` (internal resource), per-user, server-synced.
 */
interface ConsoleHistoryEntryResource {
  [key: string]: unknown;
  resourceType: "ConsoleHistoryEntry";
  id?: string;
  user: string;
  method: string;
  path: string;
  targetType?: string;
  requestHeaders?: string;
  requestBody?: string;
  status?: number;
  statusText?: string;
  durationMs?: number;
  responseSize?: number;
  responseBody?: string;
  responseHeaders?: string;
  serverTiming?: string;
  executedAt: string;
  pinned?: boolean;
  note?: string;
  collection?: string;
  mode?: string;
  tag?: string[];
}

/**
 * Current user id, injected from the auth layer (see useHistory). Used as the `user`
 * field on writes and the search filter on reads so history is scoped per-user.
 */
let currentUserId: string = ANONYMOUS_USER;

export function setHistoryUser(userId: string | null | undefined): void {
  currentUserId = userId || ANONYMOUS_USER;
}

function safeStringify(value: unknown): string | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return undefined;
  }
}

function safeParse(value: string | undefined): unknown {
  if (value === undefined) return undefined;
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function toResource(
  entry: Omit<HistoryEntry, "id" | "timestamp">,
  id: string
): ConsoleHistoryEntryResource {
  // Truncate large responses
  let responseBody = entry.responseBody;
  if (responseBody) {
    const bodySize = JSON.stringify(responseBody).length;
    if (bodySize > RESPONSE_SIZE_LIMIT) {
      responseBody = {
        __truncated: true,
        __originalSize: bodySize,
        __message: "Response truncated (>256KB). Check response in real-time viewer.",
      };
    }
  }

  const responseBodyStr = safeStringify(responseBody);

  return {
    resourceType: "ConsoleHistoryEntry",
    id,
    user: currentUserId,
    method: entry.method,
    path: entry.path,
    targetType: entry.resourceType,
    requestHeaders: safeStringify(entry.headers),
    requestBody: entry.body,
    status: entry.responseStatus,
    statusText: entry.responseStatusText,
    durationMs: entry.responseDurationMs,
    responseSize: responseBodyStr ? responseBodyStr.length : undefined,
    responseBody: responseBodyStr,
    responseHeaders: safeStringify(entry.responseHeaders),
    executedAt: entry.requestedAt,
    pinned: entry.isPinned ?? false,
    note: entry.note,
    mode: entry.mode,
    tag: entry.tags,
  };
}

function fromResource(r: ConsoleHistoryEntryResource): HistoryEntry {
  const timestamp = r.executedAt ? Date.parse(r.executedAt) : Date.now();
  return {
    id: r.id ?? crypto.randomUUID(),
    method: r.method,
    path: r.path,
    body: r.requestBody,
    headers: safeParse(r.requestHeaders) as Record<string, string> | undefined,
    requestedAt: r.executedAt,
    responseStatus: r.status,
    responseStatusText: r.statusText,
    responseDurationMs: r.durationMs,
    responseBody: safeParse(r.responseBody),
    responseHeaders: safeParse(r.responseHeaders) as Record<string, string> | undefined,
    isPinned: r.pinned ?? false,
    tags: r.tag,
    note: r.note,
    resourceType: r.targetType,
    timestamp: Number.isNaN(timestamp) ? Date.now() : timestamp,
    mode: r.mode as HistoryEntry["mode"],
  };
}

/**
 * Server-backed history service. Persists each executed request as a
 * `ConsoleHistoryEntry` FHIR resource (octofhir-console IG) so history follows the
 * user across browsers. Preserves the original public API used by the console hooks.
 */
export class HistoryService {
  async addEntry(entry: Omit<HistoryEntry, "id" | "timestamp">): Promise<string> {
    const id = crypto.randomUUID();
    const resource = toResource(entry, id);
    const created = await fhirClient.create(resource);
    return (created as unknown as ConsoleHistoryEntryResource).id ?? id;
  }

  async getAll(limit = 100): Promise<HistoryEntry[]> {
    const bundle = await fhirClient.search<ConsoleHistoryEntryResource>("ConsoleHistoryEntry", {
      user: currentUserId,
      _count: limit * 2,
      _sort: "-executedAt",
    });
    const entries = (bundle.entry ?? [])
      .map((e) => e.resource)
      .filter((r): r is ConsoleHistoryEntryResource => !!r)
      .map(fromResource);

    // Pinned first, then newest
    const sorted = entries.sort((a, b) => {
      if (a.isPinned === b.isPinned) return b.timestamp - a.timestamp;
      return a.isPinned ? -1 : 1;
    });
    return sorted.slice(0, limit);
  }

  async search(query: string): Promise<HistoryEntry[]> {
    const lowerQuery = query.toLowerCase();
    const all = await this.getAll(MAX_HISTORY_ENTRIES);
    return all.filter(
      (entry) =>
        entry.path.toLowerCase().includes(lowerQuery) ||
        entry.resourceType?.toLowerCase().includes(lowerQuery) ||
        entry.method.toLowerCase().includes(lowerQuery)
    );
  }

  async togglePin(id: string): Promise<void> {
    const resource = await fhirClient.read<ConsoleHistoryEntryResource>("ConsoleHistoryEntry", id);
    await fhirClient.update({ ...resource, pinned: !resource.pinned });
  }

  async deleteEntry(id: string): Promise<void> {
    await fhirClient.delete("ConsoleHistoryEntry", id);
  }

  async clearAll(): Promise<void> {
    const all = await this.getAll(MAX_HISTORY_ENTRIES);
    const unpinned = all.filter((e) => !e.isPinned);
    await Promise.all(unpinned.map((e) => fhirClient.delete("ConsoleHistoryEntry", e.id)));
  }

  async addNote(id: string, note: string): Promise<void> {
    const resource = await fhirClient.read<ConsoleHistoryEntryResource>("ConsoleHistoryEntry", id);
    await fhirClient.update({ ...resource, note });
  }

  async exportAll(): Promise<string> {
    const entries = await this.getAll(1000);
    return JSON.stringify(entries, null, 2);
  }
}

export const historyService = new HistoryService();

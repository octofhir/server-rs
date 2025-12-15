import Dexie, { type Table } from "dexie";

export interface HistoryEntry {
	id: string; // UUID
	method: string;
	path: string;
	body?: string;
	headers?: Record<string, string>;
	requestedAt: string; // ISO timestamp
	responseStatus?: number;
	responseStatusText?: string;
	responseDurationMs?: number;
	responseBody?: unknown;
	responseHeaders?: Record<string, string>;
	isPinned: boolean;
	tags?: string[];
	note?: string;
	resourceType?: string; // Extracted from path or body
	timestamp: number; // Unix timestamp for sorting
	mode?: "smart" | "raw"; // Console mode when request was made
}

export class HistoryDatabase extends Dexie {
	history!: Table<HistoryEntry, string>;

	constructor() {
		super("octofhir-console-history");

		this.version(1).stores({
			history: `id, method, path, resourceType, timestamp, isPinned, [isPinned+timestamp]`,
		});

		// Version 2: Add mode field
		this.version(2).stores({
			history: `id, method, path, resourceType, timestamp, isPinned, [isPinned+timestamp]`,
		});
	}
}

export const historyDb = new HistoryDatabase();

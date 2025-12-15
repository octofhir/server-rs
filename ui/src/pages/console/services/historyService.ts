import { historyDb, type HistoryEntry } from "../db/historyDatabase";

const MAX_HISTORY_ENTRIES = 200;
const RESPONSE_SIZE_LIMIT = 256 * 1024; // 256KB

export class HistoryService {
	/**
	 * Add entry to history with automatic cleanup
	 */
	async addEntry(entry: Omit<HistoryEntry, "id" | "timestamp">): Promise<string> {
		const id = crypto.randomUUID();
		const timestamp = Date.now();

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

		const fullEntry: HistoryEntry = {
			...entry,
			id,
			timestamp,
			responseBody,
			isPinned: false,
		};

		await historyDb.history.add(fullEntry);
		await this.cleanup();

		return id;
	}

	/**
	 * Get all entries, pinned first
	 */
	async getAll(limit = 100): Promise<HistoryEntry[]> {
		// Get all entries sorted by timestamp (newest first)
		const all = await historyDb.history
			.orderBy("timestamp")
			.reverse()
			.limit(limit * 2) // Get more to account for pinned filtering
			.toArray();

		// Sort: pinned items first, then by timestamp descending
		const sorted = all.sort((a, b) => {
			if (a.isPinned === b.isPinned) {
				return b.timestamp - a.timestamp;
			}
			return a.isPinned ? -1 : 1;
		});

		return sorted.slice(0, limit);
	}

	/**
	 * Search entries by path or resource type
	 */
	async search(query: string): Promise<HistoryEntry[]> {
		const lowerQuery = query.toLowerCase();

		return await historyDb.history
			.filter((entry) => {
				return (
					entry.path.toLowerCase().includes(lowerQuery) ||
					entry.resourceType?.toLowerCase().includes(lowerQuery) ||
					entry.method.toLowerCase().includes(lowerQuery)
				);
			})
			.reverse()
			.toArray();
	}

	/**
	 * Toggle pin status
	 */
	async togglePin(id: string): Promise<void> {
		const entry = await historyDb.history.get(id);
		if (!entry) return;

		await historyDb.history.update(id, {
			isPinned: !entry.isPinned,
		});
	}

	/**
	 * Delete single entry
	 */
	async deleteEntry(id: string): Promise<void> {
		await historyDb.history.delete(id);
	}

	/**
	 * Clear all history (except pinned)
	 */
	async clearAll(): Promise<void> {
		const unpinned = await historyDb.history
			.filter((entry) => !entry.isPinned)
			.toArray();

		await historyDb.history.bulkDelete(unpinned.map((e) => e.id));
	}

	/**
	 * Add note to entry
	 */
	async addNote(id: string, note: string): Promise<void> {
		await historyDb.history.update(id, { note });
	}

	/**
	 * Export history as JSON
	 */
	async exportAll(): Promise<string> {
		const entries = await this.getAll(1000);
		return JSON.stringify(entries, null, 2);
	}

	/**
	 * Cleanup old entries (keep last 200, always keep pinned)
	 */
	private async cleanup(): Promise<void> {
		const unpinned = await historyDb.history
			.filter((entry) => !entry.isPinned)
			.reverse()
			.toArray();

		if (unpinned.length > MAX_HISTORY_ENTRIES) {
			const toDelete = unpinned.slice(MAX_HISTORY_ENTRIES);
			await historyDb.history.bulkDelete(toDelete.map((e) => e.id));
		}
	}
}

export const historyService = new HistoryService();

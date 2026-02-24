import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { historyService } from "../services/historyService";
import type { HistoryEntry } from "../db/historyDatabase";
import { notifications } from "@octofhir/ui-kit";

const HISTORY_QUERY_KEY = ["console-history", "list"];

export function useHistory() {
	const queryClient = useQueryClient();

	// Query: Get all history
	const historyQuery = useQuery({
		queryKey: HISTORY_QUERY_KEY,
		queryFn: async () => {
			console.log("🔍 Fetching history from IndexedDB...");
			const entries = await historyService.getAll();
			console.log(`📊 Loaded ${entries.length} history entries from IndexedDB`);
			return entries;
		},
		staleTime: 30000, // 30s
	});

	// Mutation: Add entry
	const addMutation = useMutation({
		mutationFn: async (entry: Omit<HistoryEntry, "id" | "timestamp">) => {
			console.log("📝 Adding entry to history via mutation...", entry.path);
			const id = await historyService.addEntry(entry);
			console.log("✅ Entry added with ID:", id);
			return id;
		},
		onSuccess: async () => {
			console.log("🔄 Invalidating history query...");
			await queryClient.invalidateQueries({ queryKey: HISTORY_QUERY_KEY });
			console.log("✅ History query invalidated");
		},
	});

	// Mutation: Toggle pin
	const togglePinMutation = useMutation({
		mutationFn: (id: string) => historyService.togglePin(id),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: HISTORY_QUERY_KEY });
		},
	});

	// Mutation: Delete entry
	const deleteMutation = useMutation({
		mutationFn: (id: string) => historyService.deleteEntry(id),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: HISTORY_QUERY_KEY });
			notifications.show({
				title: "Entry Deleted",
				message: "History entry removed",
				color: "blue",
			});
		},
	});

	// Mutation: Clear all
	const clearAllMutation = useMutation({
		mutationFn: () => historyService.clearAll(),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: HISTORY_QUERY_KEY });
			notifications.show({
				title: "History Cleared",
				message: "All unpinned entries removed",
				color: "blue",
			});
		},
	});

	// Mutation: Add note
	const addNoteMutation = useMutation({
		mutationFn: ({ id, note }: { id: string; note: string }) =>
			historyService.addNote(id, note),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: HISTORY_QUERY_KEY });
		},
	});

	return {
		entries: historyQuery.data ?? [],
		isLoading: historyQuery.isLoading,
		addEntry: addMutation.mutateAsync,
		togglePin: togglePinMutation.mutate,
		deleteEntry: deleteMutation.mutate,
		clearAll: clearAllMutation.mutate,
		addNote: addNoteMutation.mutate,
	};
}

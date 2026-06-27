import { notifications } from "@octofhir/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useAuth } from "../../../shared/api/hooks/useAuth";
import type { HistoryEntry } from "../db/historyDatabase";
import { historyService, setHistoryUser } from "../services/historyService";

const HISTORY_QUERY_KEY = ["console-history", "list"];

export function useHistory() {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  const userId = user?.sub ?? "anonymous";

  // Keep the service's user scope in sync with the logged-in user.
  setHistoryUser(userId);

  // Query: Get all history (server-backed, per-user)
  const historyQuery = useQuery({
    queryKey: [...HISTORY_QUERY_KEY, userId],
    queryFn: async () => {
      return historyService.getAll();
    },
    staleTime: 30000, // 30s
  });

  // Mutation: Add entry
  const addMutation = useMutation({
    mutationFn: async (entry: Omit<HistoryEntry, "id" | "timestamp">) => {
      return historyService.addEntry(entry);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: HISTORY_QUERY_KEY });
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
    mutationFn: ({ id, note }: { id: string; note: string }) => historyService.addNote(id, note),
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

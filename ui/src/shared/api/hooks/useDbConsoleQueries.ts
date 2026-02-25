import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { serverApi } from "../serverApi";
import type { SaveHistoryRequest, TerminateQueryRequest } from "../types";

export const dbConsoleKeys = {
	all: ["dbConsole"] as const,
	history: () => [...dbConsoleKeys.all, "history"] as const,
	tables: () => [...dbConsoleKeys.all, "tables"] as const,
	tableDetail: (schema: string, table: string) => [...dbConsoleKeys.all, "tableDetail", schema, table] as const,
	activeQueries: () => [...dbConsoleKeys.all, "activeQueries"] as const,
};

export function useQueryHistory() {
	return useQuery({
		queryKey: dbConsoleKeys.history(),
		queryFn: () => serverApi.getQueryHistory(),
		staleTime: 1000 * 30,
		refetchOnWindowFocus: true,
	});
}

export function useSaveHistory() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: (request: SaveHistoryRequest) => serverApi.saveQueryHistory(request),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: dbConsoleKeys.history() });
		},
	});
}

export function useClearHistory() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: () => serverApi.clearQueryHistory(),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: dbConsoleKeys.history() });
		},
	});
}

export function useDbTables() {
	return useQuery({
		queryKey: dbConsoleKeys.tables(),
		queryFn: () => serverApi.getDbTables(),
		staleTime: 1000 * 60 * 5,
	});
}

export function useTableDetail(schema: string | undefined, table: string | undefined) {
	return useQuery({
		queryKey: dbConsoleKeys.tableDetail(schema ?? "", table ?? ""),
		queryFn: () => serverApi.getTableDetail(schema!, table!),
		enabled: !!schema && !!table,
		staleTime: 1000 * 60 * 5,
	});
}

export function useActiveQueries(enabled = true) {
	return useQuery({
		queryKey: dbConsoleKeys.activeQueries(),
		queryFn: () => serverApi.getActiveQueries(),
		refetchInterval: enabled ? 3000 : false,
		staleTime: 1000,
		enabled,
	});
}

export function useTerminateQuery() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: (request: TerminateQueryRequest) => serverApi.terminateQuery(request),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: dbConsoleKeys.activeQueries() });
		},
	});
}

export function useDropIndex() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: ({ schema, indexName }: { schema: string; indexName: string }) =>
			serverApi.dropIndex(schema, indexName),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: dbConsoleKeys.tables() });
			queryClient.invalidateQueries({ queryKey: [...dbConsoleKeys.all, "tableDetail"] });
		},
	});
}

import { useMemo } from "react";
import { useQuery, type UseQueryResult } from "@tanstack/react-query";
import { serverApi } from "@/shared/api";
import type {
	RestConsoleResponse,
	RestConsoleSearchParam,
	AutocompleteSuggestion,
} from "@/shared/api";

const restConsoleKeys = {
	all: ["rest-console"] as const,
	meta: () => [...restConsoleKeys.all, "meta"] as const,
};

type QueryResult = UseQueryResult<RestConsoleResponse>;

export interface RestConsoleMetaHookResult extends Omit<QueryResult, "data"> {
	data: RestConsoleResponse | undefined;
	resourceTypes: string[];
	allSuggestions: AutocompleteSuggestion[];
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
}

/**
 * Fetches REST console metadata from the unified introspection endpoint.
 * Returns autocomplete suggestions, search params, enriched capabilities
 * (chains, includes, special params) — all in a single request.
 */
export function useRestConsoleMeta(): RestConsoleMetaHookResult {
	const query = useQuery({
		queryKey: restConsoleKeys.meta(),
		queryFn: () => serverApi.getRestConsoleMetadata(),
		staleTime: 1000 * 60,
	});

	const payload = query.data;

	const helpers = useMemo(() => {
		if (!payload || !payload.suggestions) {
			return {
				resourceTypes: [] as string[],
				allSuggestions: [] as AutocompleteSuggestion[],
				searchParamsByResource: {} as Record<string, RestConsoleSearchParam[]>,
			};
		}

		const resourceTypes = payload.suggestions.resources.map((s) => s.label);

		const allSuggestions = [
			...payload.suggestions.resources,
			...payload.suggestions.system_operations,
			...payload.suggestions.type_operations,
			...payload.suggestions.instance_operations,
			...payload.suggestions.api_endpoints,
		];

		return {
			resourceTypes,
			allSuggestions,
			searchParamsByResource: payload.search_params || {},
		};
	}, [payload]);

	return {
		...query,
		data: payload,
		resourceTypes: helpers.resourceTypes,
		allSuggestions: helpers.allSuggestions,
		searchParamsByResource: helpers.searchParamsByResource,
	};
}

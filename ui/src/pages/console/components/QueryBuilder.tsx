import { useState, useMemo, useCallback } from "react";
import {
	TextInput,
	Textarea,
	Stack,
	Flex,
	Text,
	SegmentedRadioGroup,
	Badge,
} from "@/shared/ui";
import { useUnit } from "effector-react";
import {
	$queryParams,
	$searchParams,
	setQueryParams,
} from "../state/consoleStore";
import {
	parseQueryString,
	mergeSearchParamsAndQuery,
} from "../utils/queryParser";
import type { RestConsoleSearchParam } from "@/shared/api";

interface QueryBuilderProps {
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
	resourceType?: string;
}

type QueryBuilderMode = "builder" | "raw";

const MODE_OPTIONS = [
	{ label: "Builder", value: "builder" },
	{ label: "Raw", value: "raw" },
];

export function QueryBuilder({
	searchParamsByResource: _searchParamsByResource,
	resourceType: _resourceType,
}: QueryBuilderProps) {
	const {
		searchParams,
		queryParams,
		setQueryParams: setQueryParamsEvent,
	} = useUnit({
		searchParams: $searchParams,
		queryParams: $queryParams,
		setQueryParams,
	});

	const [mode, setMode] = useState<QueryBuilderMode>("builder");
	const [inputValue, setInputValue] = useState("");

	// Get current query string from store
	const queryString = useMemo(() => {
		return mergeSearchParamsAndQuery(searchParams, queryParams);
	}, [searchParams, queryParams]);

	const handleInputChange = useCallback(
		(value: string) => {
			setInputValue(value);
		},
		[],
	);

	const handleBlur = useCallback(() => {
		// Parse and sync to store
		const parsed = parseQueryString(inputValue || queryString);
		setQueryParamsEvent(parsed);
	}, [inputValue, queryString, setQueryParamsEvent]);

	const handleRawChange = useCallback(
		(value: string) => {
			const parsed = parseQueryString(value);
			setQueryParamsEvent(parsed);
		},
		[setQueryParamsEvent],
	);

	return (
		<Stack gap="xs">
			<Flex justifyContent="space-between" alignItems="center">
				<Text variant="subheader-1">
					Query Parameters
				</Text>
				<SegmentedRadioGroup
					size="s"
					options={MODE_OPTIONS}
					value={mode}
					onChange={(value) => setMode(value as QueryBuilderMode)}
				/>
			</Flex>

			{mode === "builder" ? (
				<TextInput
					placeholder="name=John&birthdate:ge=2000-01-01&_count=10"
					value={inputValue || queryString}
					onChange={(e) => handleInputChange(e.target.value)}
					onBlur={handleBlur}
					size="m"
					hasClear
				/>
			) : (
				<Textarea
					placeholder="name=John&birthdate:ge=2000-01-01"
					value={queryString}
					onChange={(e) => handleRawChange(e.target.value)}
					minRows={2}
					size="m"
				/>
			)}

			<Flex gap="2" alignItems="center">
				<Badge size="s" theme="info">
					{searchParams.length + Object.keys(queryParams).length} parameters
				</Badge>
				<Text variant="caption-1" color="secondary">
					Press Enter or blur to apply changes
				</Text>
			</Flex>
		</Stack>
	);
}

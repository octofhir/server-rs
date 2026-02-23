import { useCallback, useEffect, useMemo, useRef } from "react";
import { useUnit } from "effector-react";
import { Badge, Group, Stack, Text } from "@/shared/ui";
import { $method, $rawPath, setRawPath } from "../state/consoleStore";
import type {
	AutocompleteSuggestion,
	RestConsoleResponse,
	RestConsoleSearchParam,
} from "@/shared/api";
import { parseQueryAst } from "@/shared/fhir-query-input";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import {
	astToBuilderState,
	builderStateToRaw,
	type BuilderState,
} from "@/shared/fhir-query-input/core/builder-model";
import { QueryChipsBuilder } from "@/shared/fhir-query-input/widgets/QueryChipsBuilder";
import { useState } from "react";

interface BuilderModeEditorProps {
	allSuggestions: AutocompleteSuggestion[];
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
	capabilities?: RestConsoleResponse;
	isLoading?: boolean;
}

export function BuilderModeEditor({
	allSuggestions,
	searchParamsByResource,
	capabilities,
	isLoading: _isLoading,
}: BuilderModeEditorProps) {
	const {
		method,
		rawPath,
		setRawPath: setRawPathEvent,
	} = useUnit({
		method: $method,
		rawPath: $rawPath,
		setRawPath,
	});

	const metadata: QueryInputMetadata = useMemo(
		() => ({
			resourceTypes: allSuggestions
				.filter((s) => s.kind === "resource")
				.map((s) => s.label),
			searchParamsByResource,
			allSuggestions,
			capabilities,
		}),
		[allSuggestions, searchParamsByResource, capabilities],
	);

	// Initialize builder state from current rawPath
	const [builderState, setBuilderState] = useState<BuilderState>(() => {
		const path = rawPath || "/fhir/";
		const ast = parseQueryAst(path);
		return astToBuilderState(ast);
	});

	// Sync from store when rawPath changes externally
	const isSyncingToStore = useRef(false);
	// biome-ignore lint/correctness/useExhaustiveDependencies: intentionally omit builderState to prevent loop
	useEffect(() => {
		if (isSyncingToStore.current) return;
		const path = rawPath || "/fhir/";
		const ast = parseQueryAst(path);
		const newState = astToBuilderState(ast);
		setBuilderState(newState);
	}, [rawPath]);

	const handleStateChange = useCallback(
		(newState: BuilderState) => {
			setBuilderState(newState);
			isSyncingToStore.current = true;
			const raw = builderStateToRaw(newState);
			setRawPathEvent(raw);
			requestAnimationFrame(() => {
				isSyncingToStore.current = false;
			});
		},
		[setRawPathEvent],
	);

	return (
		<Stack gap="xs">
			<Group justify="space-between">
				<Text fw={500} size="sm">
					Visual Query Builder
				</Text>
				<Badge variant="light" size="sm">
					{method}
				</Badge>
			</Group>
			<QueryChipsBuilder
				state={builderState}
				onChange={handleStateChange}
				metadata={metadata}
			/>
		</Stack>
	);
}

import { useCallback, useMemo } from "react";
import { IconPlayerPlay } from "@tabler/icons-react";
import { useUnit } from "effector-react";
import { $rawPath, setRawPath } from "../state/consoleStore";
import type {
	AutocompleteSuggestion,
	RestConsoleResponse,
	RestConsoleSearchParam,
} from "@/shared/api";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import { QueryEditor } from "@/shared/fhir-query-input/widgets/QueryEditor";
import { Box, Divider } from "@/shared/ui";
import { MethodControl } from "./MethodControl";
import { Button } from "@octofhir/ui-kit";

interface RequestBarProps {
	allSuggestions: AutocompleteSuggestion[];
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
	capabilities?: RestConsoleResponse;
	isLoading?: boolean;
	isSending?: boolean;
	onSend: () => void;
}

export function RequestBar({
	allSuggestions,
	searchParamsByResource,
	capabilities,
	isLoading,
	isSending,
	onSend,
}: RequestBarProps) {
	const { rawPath, setRawPath: setRawPathEvent } = useUnit({
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

	const handleChange = useCallback(
		(value: string) => {
			setRawPathEvent(value);
		},
		[setRawPathEvent],
	);

	return (
		<Box
			style={{
				display: "flex",
				alignItems: "flex-start",
				border: "1px solid var(--octo-border-subtle)",
				borderRadius: "var(--mantine-radius-md)",
				backgroundColor: "var(--octo-surface-1)",
				overflow: "visible",
			}}
		>
			<MethodControl />
			<Divider orientation="vertical" style={{ alignSelf: "stretch" }} />
			<Box style={{ flex: 1, minWidth: 0 }}>
				<QueryEditor
					value={rawPath}
					onChange={handleChange}
					onExecute={onSend}
					metadata={metadata}
					disabled={isLoading}
					borderless
				/>
			</Box>
			<Divider orientation="vertical" style={{ alignSelf: "stretch" }} />
			<Button
				onClick={onSend}
				loading={isSending}
				disabled={!rawPath}
				rightSection={<IconPlayerPlay size={14} />}
			>
				Send
			</Button>
		</Box>
	);
}

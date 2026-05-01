import { useCallback, useMemo } from "react";
import { Play } from "@gravity-ui/icons";
import { useUnit } from "effector-react";
import { $rawPath, setRawPath } from "../state/consoleStore";
import type {
	AutocompleteSuggestion,
	RestConsoleResponse,
	RestConsoleSearchParam,
} from "@/shared/api";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import { QueryEditor } from "@/shared/fhir-query-input/widgets/QueryEditor";
import { Box, Flex, Divider, Button } from "@/shared/ui";
import { MethodControl } from "./MethodControl";

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
		<Flex alignItems="center" style={{ padding: "4px" }}>
			<Box style={{ padding: "0 8px" }}>
				<MethodControl />
			</Box>
			<Divider orientation="vertical" style={{ height: "24px" }} />
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
			<Box style={{ paddingLeft: "8px" }}>
				<Button
					view="action"
					size="l"
					onClick={onSend}
					loading={isSending}
					disabled={!rawPath}
				>
					<Button.Icon><Play size={18} /></Button.Icon>
					Send
				</Button>
			</Box>
		</Flex>
	);
}

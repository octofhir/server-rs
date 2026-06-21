import { Button } from "@octofhir/ui-kit";
import { useCallback, useMemo } from "react";
import { Play } from "lucide-react";
import { useUnit } from "effector-react";
import { $rawPath, setRawPath } from "../state/consoleStore";
import type {
	AutocompleteSuggestion,
	RestConsoleResponse,
	RestConsoleSearchParam,
} from "@/shared/api";
import type { QueryInputMetadata } from "@/shared/fhir-query-input";
import { QueryEditor } from "@/shared/fhir-query-input/widgets/QueryEditor";
import { MethodControl } from "./MethodControl";
import styles from "./RequestBar.module.css";

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
		<div className={styles.root}>
			<div className={styles.method}>
				<MethodControl />
			</div>
			<div className={styles.divider} />
			<div className={styles.query}>
				<QueryEditor
					value={rawPath}
					onChange={handleChange}
					onExecute={onSend}
					metadata={metadata}
					disabled={isLoading}
					borderless
				/>
			</div>
			<div className={styles.actions}>
				<Button
					variant="filled"
					size="lg"
					onClick={onSend}
					loading={isSending}
					disabled={!rawPath}
				>
					<Button.Icon><Play size={18} /></Button.Icon>
					Send
				</Button>
			</div>
		</div>
	);
}

import { SegmentedControl } from "@mantine/core";
import type { HttpMethod } from "@/shared/api";
import { useConsoleStore } from "../state/consoleStore";

const METHOD_OPTIONS: Array<{ label: string; value: HttpMethod }> = [
	{ label: "GET", value: "GET" },
	{ label: "POST", value: "POST" },
	{ label: "PUT", value: "PUT" },
	{ label: "PATCH", value: "PATCH" },
	{ label: "DELETE", value: "DELETE" },
	{ label: "HEAD", value: "HEAD" },
	{ label: "OPTIONS", value: "OPTIONS" },
];

export function MethodControl() {
	const method = useConsoleStore((state) => state.method);
	const setMethod = useConsoleStore((state) => state.setMethod);

	return (
		<SegmentedControl
			fullWidth
			value={method}
			onChange={(value) => setMethod(value as HttpMethod)}
			data={METHOD_OPTIONS}
		/>
	);
}

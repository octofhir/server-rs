import { SegmentedControl } from "@mantine/core";
import type { HttpMethod } from "@/shared/api";
import { useUnit } from "effector-react";
import { $method, setMethod } from "../state/consoleStore";

const METHOD_OPTIONS: Array<{ label: string; value: HttpMethod }> = [
	{ label: "GET", value: "GET" },
	{ label: "POST", value: "POST" },
	{ label: "PUT", value: "PUT" },
	{ label: "PATCH", value: "PATCH" },
	{ label: "DELETE", value: "DELETE" },
	{ label: "HEAD", value: "HEAD" },
	{ label: "OPTIONS", value: "OPTIONS" },
];

export const MethodControl = () => {
	const { method, setMethod: setMethodEvent } = useUnit({
		method: $method,
		setMethod,
	});

	return (
		<SegmentedControl
			key="method-control"
			fullWidth
			value={method}
			onChange={(value) => setMethodEvent(value as HttpMethod)}
			data={METHOD_OPTIONS}
		/>
	);
};

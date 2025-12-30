import { SegmentedControl } from "@mantine/core";
import type { HttpMethod } from "@/shared/api";
import { useUnit } from "effector-react";
import { $method, setMethod } from "../state/consoleStore";

const METHOD_OPTIONS = [
	"GET",
	"POST",
	"PUT",
	"PATCH",
	"DELETE",
	"HEAD",
	"OPTIONS",
];

export const MethodControl = () => {
	const { method, setMethod: setMethodEvent } = useUnit({
		method: $method,
		setMethod,
	});

	return (
		<SegmentedControl
			fullWidth
			value={method}
			onChange={(value) => setMethodEvent(value as HttpMethod)}
			data={METHOD_OPTIONS}
		/>
	);
};

import { Select } from "@mantine/core";
import type { HttpMethod } from "@/shared/api";
import { useUnit } from "effector-react";
import { $method, setMethod } from "../state/consoleStore";

const METHOD_OPTIONS = ["GET", "POST", "PUT", "PATCH", "DELETE"];

export function MethodControl() {
	const { method, setMethod: setMethodEvent } = useUnit({
		method: $method,
		setMethod,
	});

	return (
		<Select
			value={method}
			onChange={(v) => setMethodEvent((v ?? "GET") as HttpMethod)}
			data={METHOD_OPTIONS}
			size="sm"
			variant="unstyled"
			allowDeselect={false}
			withCheckIcon={false}
			styles={{
				input: {
					fontWeight: 700,
					fontFamily: "var(--font-mono, 'JetBrains Mono', monospace)",
					width: 90,
					paddingLeft: 12,
					paddingRight: 4,
					fontSize: 13,
				},
			}}
		/>
	);
}

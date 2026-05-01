import { Select } from "@/shared/ui";
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
			value={[method]}
			onUpdate={(v) => setMethodEvent((v[0] ?? "GET") as HttpMethod)}
			options={METHOD_OPTIONS.map(m => ({ value: m, content: m }))}
			size="m"
			view="flat"
			style={{ width: 80, fontWeight: 700 }}
		/>
	);
}

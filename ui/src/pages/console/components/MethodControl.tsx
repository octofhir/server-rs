import { Select } from "@octofhir/ui-kit";
import { isHttpMethod } from "@/shared/api";
import { useUnit } from "effector-react";
import { $method, setMethod } from "../state/consoleStore";
import styles from "./MethodControl.module.css";

const METHOD_OPTIONS = ["GET", "POST", "PUT", "PATCH", "DELETE"];

export function MethodControl() {
	const { method, setMethod: setMethodEvent } = useUnit({
		method: $method,
		setMethod,
	});

	return (
		<Select
			value={method}
			onUpdate={(v) => {
				setMethodEvent(v && isHttpMethod(v) ? v : "GET");
			}}
			options={METHOD_OPTIONS.map(m => ({ value: m, content: m }))}
			size="md"
			className={styles.select}
		/>
	);
}

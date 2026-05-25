import { Text, TextInput } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { $rawPath, setRawPath } from "../state/consoleStore";
import styles from "./RawRequestInput.module.css";

export function RawRequestInput() {
	const { rawPath, setRawPath: setRawPathEvent } = useUnit({
		rawPath: $rawPath,
		setRawPath,
	});

	return (
		<div className={styles.root}>
			<Text fw={500} size="sm">
				Raw Request Path
			</Text>
			<TextInput
				placeholder="/fhir/Patient?name=John&_count=10"
				value={rawPath}
				onChange={(e) => setRawPathEvent(e.target.value)}
				size="sm"
			/>
			<Text size="xs" className={styles.hint}>
				Enter the full request path including /fhir prefix and query parameters
			</Text>
		</div>
	);
}

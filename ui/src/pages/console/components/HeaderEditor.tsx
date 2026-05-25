import { ActionIcon, Alert, Button, IconLock, IconPlus, IconX, Text, TextInput } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import {
	$customHeaders,
	$defaultHeaders,
	addCustomHeader,
	removeCustomHeader,
	updateCustomHeader,
} from "../state/consoleStore";
import { validateHeaders } from "../utils/headerUtils";
import styles from "./HeaderEditor.module.css";

export function HeaderEditor() {
	const {
		defaultHeaders,
		customHeaders,
		addCustomHeader: addCustomHeaderEvent,
		removeCustomHeader: removeCustomHeaderEvent,
		updateCustomHeader: updateCustomHeaderEvent,
	} = useUnit({
		defaultHeaders: $defaultHeaders,
		customHeaders: $customHeaders,
		addCustomHeader,
		removeCustomHeader,
		updateCustomHeader,
	});

	const allHeaders = { ...defaultHeaders, ...customHeaders };
	const errors = validateHeaders(allHeaders);

	const handleAddHeader = () => {
		addCustomHeaderEvent({ key: "", value: "" });
	};

	return (
		<div className={styles.root}>
			<Text fw={500} size="sm">
				Headers
			</Text>

			{/* Default headers */}
			<div className={styles.section}>
				<Text size="xs" c="dimmed">
					Default FHIR headers
				</Text>
				{Object.entries(defaultHeaders).map(([key, value]) => (
					<div key={key} className={styles.row}>
						<TextInput value={key} disabled size="xs" />
						<TextInput value={value} disabled size="xs" />
						<ActionIcon variant="subtle" disabled size="sm">
							<IconLock size={14} />
						</ActionIcon>
					</div>
				))}
			</div>

			{/* Custom headers */}
			<div className={styles.section}>
				<div className={styles.sectionHeader}>
					<Text size="xs" c="dimmed">
						Custom headers
					</Text>
					<Button size="xs" leftSection={<IconPlus size={14} />} onClick={handleAddHeader}>
						Add
					</Button>
				</div>
				{Object.entries(customHeaders).map(([key, value]) => (
					<div key={key} className={styles.row}>
						<TextInput
							placeholder="Header-Name"
							value={key}
							onChange={(e) =>
								updateCustomHeaderEvent({
									oldKey: key,
									newKey: e.target.value,
									value,
								})
							}
							size="xs"
						/>
						<TextInput
							placeholder="value"
							value={value}
							onChange={(e) =>
								updateCustomHeaderEvent({
									oldKey: key,
									newKey: key,
									value: e.target.value,
								})
							}
							size="xs"
						/>
						<ActionIcon
							variant="subtle"
							onClick={() => removeCustomHeaderEvent(key)}
							size="sm"
							color="fire"
						>
							<IconX size={14} />
						</ActionIcon>
					</div>
				))}
			</div>

			{/* Validation warnings */}
			{errors.length > 0 && (
				<Alert color="warm" title="Validation warnings">
					{errors.map((error, i) => (
						<Text key={i} size="xs">
							{error}
						</Text>
					))}
				</Alert>
			)}

			<Text size="xs" className={styles.footer}>
				{Object.keys(customHeaders).length} custom headers
			</Text>
		</div>
	);
}

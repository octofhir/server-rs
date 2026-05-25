import { Alert, Button, IconAlertCircle, Skeleton, Text } from "@octofhir/ui-kit";
import { Suspense, useState } from "react";
import { useUnit } from "effector-react";
import { JsonEditor } from "@/shared/monaco";
import { $body, setBody } from "../state/consoleStore";
import { generateTemplate } from "../utils/templateGenerator";
import { useJsonSchema } from "@/shared/api/hooks";
import { isRecord } from "@/shared/api/guards";
import styles from "./BodyEditor.module.css";

interface BodyEditorProps {
	resourceType?: string;
	method: string;
}

export function BodyEditor({ resourceType, method }: BodyEditorProps) {
	const { body, setBody: setBodyEvent } = useUnit({ body: $body, setBody });
	const [validationError, setValidationError] = useState<string>();
	const { data: jsonSchema } = useJsonSchema(resourceType);

	const showBodyEditor = ["POST", "PUT", "PATCH"].includes(method);
	const canInsertTemplate = resourceType && showBodyEditor;
	const editorSchema = isRecord(jsonSchema) ? jsonSchema : undefined;

	const handleInsertTemplate = () => {
		if (!resourceType) return;
		const template = generateTemplate(resourceType);
		setBodyEvent(template);
	};

	const handleFormat = () => {
		try {
			const parsed = JSON.parse(body);
			setBodyEvent(JSON.stringify(parsed, null, 2));
			setValidationError(undefined);
		} catch (error) {
			setValidationError(error instanceof Error ? error.message : "Invalid JSON");
		}
	};

	if (!showBodyEditor) {
		return (
			<Text size="sm" c="dimmed">
				Request body is only available for POST, PUT, and PATCH requests
			</Text>
		);
	}

	return (
		<div className={styles.root}>
			<div className={styles.header}>
				<Text fw={500} size="sm">
					Request Body
				</Text>
				<div className={styles.actions}>
					{canInsertTemplate && (
						<Button size="xs" variant="light" onClick={handleInsertTemplate}>
							Insert template
						</Button>
					)}
					<Button size="xs" variant="subtle" onClick={handleFormat}>
						Format
					</Button>
				</div>
			</div>

			<Suspense fallback={<Skeleton height={300} />}>
				<JsonEditor
					value={body}
					onChange={setBodyEvent}
					height={300}
					onValidationError={setValidationError}
					schema={editorSchema}
					resourceType={resourceType}
				/>
			</Suspense>

			{validationError && (
				<Alert color="fire" icon={<IconAlertCircle size={16} />}>
					{validationError}
				</Alert>
			)}

			<Text size="xs" className={styles.footer}>
				{body.length} bytes
			</Text>
		</div>
	);
}

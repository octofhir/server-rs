import { Button, Modal } from "@octofhir/ui-kit";
import { useState } from "react";
import { Text, Checkbox, CopyButton, Alert } from "@octofhir/ui-kit";
import { TriangleAlert as TriangleExclamation } from "lucide-react";
import classes from "./SecretDisplayModal.module.css";

interface SecretDisplayModalProps {
	opened: boolean;
	onClose: () => void;
	clientId: string;
	clientSecret: string;
	isNewClient: boolean;
}

/**
 * Modal for displaying a newly created or regenerated client secret.
 * The secret is shown only once and must be copied by the user.
 */
export function SecretDisplayModal({
	opened,
	onClose,
	clientId,
	clientSecret,
	isNewClient,
}: SecretDisplayModalProps) {
	const [confirmed, setConfirmed] = useState(false);

	const handleClose = () => {
		setConfirmed(false);
		onClose();
	};

	return (
		<Modal
			opened={opened}
			onClose={handleClose}
			title={isNewClient ? "Client Created Successfully" : "Secret Regenerated"}
			size="lg"
			closeOnClickOutside={false}
			closeOnEscape={false}
		>
			<div className={classes.content}>
				<Alert
					icon={<TriangleExclamation size={20} />}
					color="orange"
					variant="light"
					className={classes.warningAlert}
				>
					<Text size="sm" fw={500}>
						This secret will only be shown once.
					</Text>
					<Text size="sm" c="dimmed">
						Please copy it now and store it securely. You will not be able to
						view it again.
					</Text>
				</Alert>

				<div>
					<Text className={classes.label}>Client ID</Text>
					<div className={classes.secretField}>
						<span className={classes.secretValue}>{clientId}</span>
						<CopyButton
							text={clientId}
							tooltipInitialText="Copy Client ID"
							aria-label="Copy Client ID"
						/>
					</div>
				</div>

				<div>
					<Text className={classes.label}>Client Secret</Text>
					<div className={classes.secretField}>
						<span className={classes.secretValue}>{clientSecret}</span>
						<CopyButton
							text={clientSecret}
							tooltipInitialText="Copy Secret"
							aria-label="Copy Secret"
						/>
					</div>
				</div>

				<Checkbox
					label="I have copied the secret and stored it securely"
					checked={confirmed}
					onChange={(e) => setConfirmed(e)}
				/>

				<div className={classes.actions}>
					<Button onClick={handleClose} disabled={!confirmed}>
						Close
					</Button>
				</div>
			</div>
		</Modal>
	);
}

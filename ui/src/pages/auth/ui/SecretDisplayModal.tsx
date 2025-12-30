import { useState } from "react";
import {
	Stack,
	Text,
	Checkbox,
	Group,
	CopyButton,
	ActionIcon,
	Tooltip,
	Alert,
} from "@mantine/core";
import { IconAlertTriangle, IconCopy, IconCheck } from "@tabler/icons-react";
import { Modal } from "@/shared/ui/Modal/Modal";
import { Button } from "@/shared/ui/Button/Button";
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
			<Stack gap="md">
				<Alert
					icon={<IconAlertTriangle size={20} />}
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
						<CopyButton value={clientId} timeout={2000}>
							{({ copied, copy }) => (
								<Tooltip
									label={copied ? "Copied!" : "Copy Client ID"}
									withArrow
								>
									<ActionIcon
										variant="subtle"
										color={copied ? "teal" : "gray"}
										onClick={copy}
									>
										{copied ? <IconCheck size={16} /> : <IconCopy size={16} />}
									</ActionIcon>
								</Tooltip>
							)}
						</CopyButton>
					</div>
				</div>

				<div>
					<Text className={classes.label}>Client Secret</Text>
					<div className={classes.secretField}>
						<span className={classes.secretValue}>{clientSecret}</span>
						<CopyButton value={clientSecret} timeout={2000}>
							{({ copied, copy }) => (
								<Tooltip
									label={copied ? "Copied!" : "Copy Secret"}
									withArrow
								>
									<ActionIcon
										variant="subtle"
										color={copied ? "teal" : "gray"}
										onClick={copy}
									>
										{copied ? <IconCheck size={16} /> : <IconCopy size={16} />}
									</ActionIcon>
								</Tooltip>
							)}
						</CopyButton>
					</div>
				</div>

				<Checkbox
					label="I have copied the secret and stored it securely"
					checked={confirmed}
					onChange={(e) => setConfirmed(e.currentTarget.checked)}
				/>

				<Group justify="flex-end">
					<Button onClick={handleClose} disabled={!confirmed}>
						Close
					</Button>
				</Group>
			</Stack>
		</Modal>
	);
}

import { Stack, Text, Group, Alert } from "@mantine/core";
import { IconAlertTriangle } from "@tabler/icons-react";
import { Modal } from "@/shared/ui/Modal/Modal";
import { Button } from "@/shared/ui/Button/Button";
import classes from "./DeleteClientModal.module.css";

interface DeleteClientModalProps {
	opened: boolean;
	onClose: () => void;
	onConfirm: () => void;
	clientName: string;
	clientId: string;
	isDeleting: boolean;
}

/**
 * Confirmation modal for deleting an OAuth client.
 */
export function DeleteClientModal({
	opened,
	onClose,
	onConfirm,
	clientName,
	clientId,
	isDeleting,
}: DeleteClientModalProps) {
	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title="Delete OAuth Client"
			size="md"
		>
			<Stack gap="md">
				<Text size="sm">
					You are about to delete the following OAuth client:
				</Text>

				<div className={classes.clientInfo}>
					<Stack gap="xs">
						<div>
							<Text className={classes.label}>Name</Text>
							<Text className={classes.value}>{clientName}</Text>
						</div>
						<div>
							<Text className={classes.label}>Client ID</Text>
							<Text className={classes.value}>{clientId}</Text>
						</div>
					</Stack>
				</div>

				<Alert
					icon={<IconAlertTriangle size={20} />}
					color="red"
					variant="light"
					className={classes.warningAlert}
				>
					<Text size="sm" fw={500}>
						This action cannot be undone.
					</Text>
					<Text size="sm" c="dimmed">
						All tokens issued to this client will be invalidated immediately.
					</Text>
				</Alert>

				<Group justify="flex-end" gap="sm">
					<Button variant="light" onClick={onClose} disabled={isDeleting}>
						Cancel
					</Button>
					<Button color="red" onClick={onConfirm} loading={isDeleting}>
						Delete Client
					</Button>
				</Group>
			</Stack>
		</Modal>
	);
}

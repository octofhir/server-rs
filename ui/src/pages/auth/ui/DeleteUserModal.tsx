import { Stack, Text, Group, Alert } from "@/shared/ui";
import { IconAlertTriangle } from "@tabler/icons-react";
import { Modal } from "@/shared/ui/Modal/Modal";
import { Button } from "@/shared/ui/Button/Button";
import classes from "./DeleteUserModal.module.css";

interface DeleteUserModalProps {
	opened: boolean;
	onClose: () => void;
	onConfirm: () => void;
	userName: string;
	userEmail: string;
	isDeleting: boolean;
}

export function DeleteUserModal({
	opened,
	onClose,
	onConfirm,
	userName,
	userEmail,
	isDeleting,
}: DeleteUserModalProps) {
	return (
		<Modal opened={opened} onClose={onClose} title="Delete User" size="md">
			<Stack gap="md">
				<Text size="sm">You are about to delete the following user:</Text>

				<div className={classes.userInfo}>
					<Stack gap="xs">
						<div>
							<Text className={classes.label}>Name</Text>
							<Text className={classes.value}>{userName}</Text>
						</div>
						{userEmail && (
							<div>
								<Text className={classes.label}>Email</Text>
								<Text className={classes.value}>{userEmail}</Text>
							</div>
						)}
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
						All user data, sessions, and associated records will be permanently deleted.
					</Text>
				</Alert>

				<Group justify="flex-end" gap="sm">
					<Button variant="light" onClick={onClose} disabled={isDeleting}>
						Cancel
					</Button>
					<Button color="red" onClick={onConfirm} loading={isDeleting}>
						Delete User
					</Button>
				</Group>
			</Stack>
		</Modal>
	);
}

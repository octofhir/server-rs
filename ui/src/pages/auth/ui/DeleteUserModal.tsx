import { Alert, Button, Modal, Text } from "@octofhir/ui-kit";
import { TriangleExclamation } from "@gravity-ui/icons";
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
			<div className={classes.content}>
				<Text size="sm">You are about to delete the following user:</Text>

				<div className={classes.userInfo}>
					<div className={classes.details}>
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
					</div>
				</div>

				<Alert
					icon={<TriangleExclamation size={20} />}
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

				<div className={classes.actions}>
					<Button variant="light" onClick={onClose} disabled={isDeleting}>
						Cancel
					</Button>
					<Button color="red" onClick={onConfirm} loading={isDeleting}>
						Delete User
					</Button>
				</div>
			</div>
		</Modal>
	);
}

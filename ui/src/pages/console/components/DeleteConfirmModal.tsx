import { Button, Code, Modal, Text } from "@/shared/ui";
import { useDesignTokens } from "@octofhir/ui-kit";
import { IconAlertTriangle } from "@octofhir/ui-kit";
import styles from "./DeleteConfirmModal.module.css";

interface DeleteConfirmModalProps {
	opened: boolean;
	onClose: () => void;
	onConfirm: () => void;
	path: string;
	resourceType?: string;
}

export function DeleteConfirmModal({
	opened,
	onClose,
	onConfirm,
	path,
	resourceType,
}: DeleteConfirmModalProps) {
	const theme = useDesignTokens();

	const handleConfirm = () => {
		onConfirm();
		onClose();
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={
				<span className={styles.title}>
					<IconAlertTriangle size={20} color={theme.colors.fire[6]} />
					<Text fw={600}>Confirm Deletion</Text>
				</span>
			}
			centered
			size="md"
		>
			<div className={styles.content}>
				<Text size="sm">
					You are about to delete a resource. This action cannot be undone.
				</Text>

				<div className={styles.details}>
					{resourceType && (
						<div className={styles.detailRow}>
							<Text size="sm" fw={500}>
								Resource Type:
							</Text>
							<Code>{resourceType}</Code>
						</div>
					)}

					<div className={styles.detailRow}>
						<Text size="sm" fw={500}>
							Path:
						</Text>
						<Code className={styles.path}>{path}</Code>
					</div>
				</div>

				<Text size="sm" c="dimmed">
					Are you sure you want to proceed with this DELETE request?
				</Text>

				<div className={styles.actions}>
					<Button variant="default" onClick={onClose}>
						Cancel
					</Button>
					<Button color="fire" onClick={handleConfirm}>
						Delete Resource
					</Button>
				</div>
			</div>
		</Modal>
	);
}

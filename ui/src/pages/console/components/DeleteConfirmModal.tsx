import { Button, Code, Group, Modal, Stack, Text } from "@/shared/ui";
import { useMantineTheme } from "@octofhir/ui-kit";
import { IconAlertTriangle } from "@tabler/icons-react";

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
	const theme = useMantineTheme();

	const handleConfirm = () => {
		onConfirm();
		onClose();
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={
				<Group gap="xs">
					<IconAlertTriangle size={20} color={theme.colors.fire[6]} />
					<Text fw={600}>Confirm Deletion</Text>
				</Group>
			}
			centered
			size="md"
		>
			<Stack gap="md">
				<Text size="sm">
					You are about to delete a resource. This action cannot be undone.
				</Text>

				<Stack gap="xs">
					{resourceType && (
						<Group gap="xs">
							<Text size="sm" fw={500}>
								Resource Type:
							</Text>
							<Code>{resourceType}</Code>
						</Group>
					)}

					<Group gap="xs" align="flex-start">
						<Text size="sm" fw={500}>
							Path:
						</Text>
						<Code style={{ flex: 1, wordBreak: "break-all" }}>{path}</Code>
					</Group>
				</Stack>

				<Text size="sm" c="dimmed">
					Are you sure you want to proceed with this DELETE request?
				</Text>

				<Group justify="flex-end" gap="sm">
					<Button variant="default" onClick={onClose}>
						Cancel
					</Button>
					<Button color="fire" onClick={handleConfirm}>
						Delete Resource
					</Button>
				</Group>
			</Stack>
		</Modal>
	);
}

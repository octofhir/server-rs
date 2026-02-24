import { useParams, useNavigate } from "react-router-dom";
import {
	Stack,
	Title,
	Text,
	Paper,
	Group,
	Badge,
	Loader,
	Alert,
	Button,
	Code,
	Switch,
	Textarea,
	Divider,
	Breadcrumbs,
	Anchor,
} from "@/shared/ui";
import { useMantineTheme } from "@octofhir/ui-kit";
import {
	IconAlertCircle,
	IconArrowLeft,
	IconLock,
	IconLockOpen,
	IconServer,
	IconCode,
	IconDatabase,
	IconShield,
	IconApps,
	IconApi,
} from "@tabler/icons-react";
import { useOperation, useUpdateOperation } from "@/shared/api/hooks";
import { useState, useEffect } from "react";

const CATEGORY_ICONS: Record<string, typeof IconServer> = {
	fhir: IconServer,
	graphql: IconCode,
	system: IconDatabase,
	auth: IconShield,
	ui: IconApps,
	api: IconApi,
};

const CATEGORY_COLORS: Record<string, string> = {
	fhir: "primary",
	graphql: "deep",
	system: "warm",
	auth: "fire",
	ui: "warm",
	api: "gray",
};

const CATEGORY_LABELS: Record<string, string> = {
	fhir: "FHIR REST API",
	graphql: "GraphQL",
	system: "System",
	auth: "Authentication",
	ui: "UI API",
	api: "Custom API",
};

function MethodBadge({ method }: { method: string }) {
	const colors: Record<string, string> = {
		GET: "primary",
		POST: "fire",
		PUT: "deep",
		DELETE: "fire",
		PATCH: "warm",
	};

	return (
		<Badge size="sm" variant="light" color={colors[method] ?? "gray"}>
			{method}
		</Badge>
	);
}

export function OperationDetailPage() {
	const { id } = useParams<{ id: string }>();
	const navigate = useNavigate();
	const { data: operation, isLoading, error } = useOperation(id ?? "");
	const updateMutation = useUpdateOperation();
	const theme = useMantineTheme();

	const [isPublic, setIsPublic] = useState(false);
	const [description, setDescription] = useState("");
	const [hasChanges, setHasChanges] = useState(false);

	useEffect(() => {
		if (operation) {
			setIsPublic(operation.public);
			setDescription(operation.description ?? "");
		}
	}, [operation]);

	useEffect(() => {
		if (operation) {
			const publicChanged = isPublic !== operation.public;
			const descChanged = description !== (operation.description ?? "");
			setHasChanges(publicChanged || descChanged);
		}
	}, [isPublic, description, operation]);

	const handleSave = async () => {
		if (!id || !operation) return;

		const update: { public?: boolean; description?: string } = {};
		if (isPublic !== operation.public) update.public = isPublic;
		if (description !== (operation.description ?? "")) update.description = description;

		await updateMutation.mutateAsync({ id, update });
	};

	const handleReset = () => {
		if (operation) {
			setIsPublic(operation.public);
			setDescription(operation.description ?? "");
		}
	};

	if (!id) {
		return (
			<Alert icon={<IconAlertCircle size={16} />} color="fire" variant="light">
				Operation ID is required
			</Alert>
		);
	}

	const CategoryIcon = operation ? CATEGORY_ICONS[operation.category] ?? IconApi : IconApi;
	const categoryColor = operation ? CATEGORY_COLORS[operation.category] ?? "gray" : "gray";
	const categoryLabel = operation ? CATEGORY_LABELS[operation.category] ?? operation.category : "";

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<Breadcrumbs>
				<Anchor onClick={() => navigate("/operations")}>Operations</Anchor>
				<Text>Detail</Text>
			</Breadcrumbs>

			<Group>
				<Button variant="subtle" leftSection={<IconArrowLeft size={16} />} onClick={() => navigate("/operations")}>
					Back
				</Button>
			</Group>

			{isLoading && (
				<Group justify="center" py="xl">
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading operation...
					</Text>
				</Group>
			)}

			{error && (
				<Alert icon={<IconAlertCircle size={16} />} color="fire" variant="light">
					{error instanceof Error ? error.message : "Failed to load operation"}
				</Alert>
			)}

			{operation && (
				<>
					<Paper p="lg" style={{ backgroundColor: "var(--octo-surface-1)" }}>
						<Stack gap="md">
							<Group justify="space-between" align="flex-start">
								<div>
									<Group gap="sm" mb="xs">
										<CategoryIcon size={24} color={theme.colors[categoryColor][6]} />
										<Title order={3}>{operation.name}</Title>
									</Group>
									<Code size="sm">{operation.id}</Code>
								</div>
								<Group gap="xs">
									<Badge variant="light" color={categoryColor}>
										{categoryLabel}
									</Badge>
									{operation.public ? (
										<Badge color="primary" variant="light" leftSection={<IconLockOpen size={12} />}>
											Public
										</Badge>
									) : (
										<Badge color="deep" variant="light" leftSection={<IconLock size={12} />}>
											Protected
										</Badge>
									)}
								</Group>
							</Group>

							<Divider />

							<div>
								<Text size="sm" fw={500} mb="xs">
									HTTP Methods
								</Text>
								<Group gap="xs">
									{operation.methods.map((method) => (
										<MethodBadge key={method} method={method} />
									))}
								</Group>
							</div>

							<div>
								<Text size="sm" fw={500} mb="xs">
									Path Pattern
								</Text>
								<Code block>{operation.path_pattern}</Code>
							</div>

							<div>
								<Text size="sm" fw={500} mb="xs">
									Module
								</Text>
								<Code>{operation.module}</Code>
							</div>
						</Stack>
					</Paper>

					<Paper p="lg" style={{ backgroundColor: "var(--octo-surface-2)" }}>
						<Title order={4} mb="md">
							Settings
						</Title>

						<Stack gap="md">
							<Switch
								label="Public Access"
								description="When enabled, this operation does not require authentication"
								checked={isPublic}
								onChange={(e) => setIsPublic(e.currentTarget.checked)}
							/>

							<Textarea
								label="Description"
								description="Optional description for this operation"
								placeholder="Describe what this operation does..."
								value={description}
								onChange={(e) => setDescription(e.currentTarget.value)}
								minRows={3}
								autosize
								maxRows={6}
							/>

							{hasChanges && (
								<Group>
									<Button onClick={handleSave} loading={updateMutation.isPending}>
										Save Changes
									</Button>
									<Button variant="subtle" onClick={handleReset}>
										Reset
									</Button>
								</Group>
							)}

							{updateMutation.isError && (
								<Alert icon={<IconAlertCircle size={16} />} color="fire" variant="light">
									{updateMutation.error instanceof Error ? updateMutation.error.message : "Failed to update operation"}
								</Alert>
							)}

							{updateMutation.isSuccess && (
								<Alert color="primary" variant="light">
									Operation updated successfully
								</Alert>
							)}
						</Stack>
					</Paper>
				</>
			)}
		</Stack>
	);
}

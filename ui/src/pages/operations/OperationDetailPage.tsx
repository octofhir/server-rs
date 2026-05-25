import { useParams, useNavigate } from "react-router-dom";
import {
	Text,
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
	KeyValueList,
	SectionPanel,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
	CircleExclamation,
	ArrowLeft,
	Lock,
	LockOpen,
	Server,
	Code as CodeIcon,
	Database,
	Shield,
	Boxes3,
	Cpu,
} from "@gravity-ui/icons";
import {
	getOperationAccessView,
	getOperationCategoryView,
	getOperationMethodView,
} from "@/entities/operation-catalog";
import { useOperation, useUpdateOperation } from "@/shared/api/hooks";
import { useState, useEffect } from "react";
import classes from "./OperationDetailPage.module.css";

const CATEGORY_ICONS: Record<string, typeof Server> = {
	fhir: Server,
	graphql: CodeIcon,
	system: Database,
	auth: Shield,
	ui: Boxes3,
	api: Cpu,
};

function MethodBadge({ method }: { method: string }) {
	const methodView = getOperationMethodView(method);

	return (
		<Badge size="sm" variant="light" color={methodView.color}>
			{methodView.method}
		</Badge>
	);
}

export function OperationDetailPage() {
	const { id } = useParams<{ id: string }>();
	const navigate = useNavigate();
	const { data: operation, isLoading, error } = useOperation(id ?? "");
	const updateMutation = useUpdateOperation();

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
			<Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
				Operation ID is required
			</Alert>
		);
	}

	const CategoryIcon = operation ? CATEGORY_ICONS[operation.category] ?? Cpu : Cpu;
	const categoryView = operation ? getOperationCategoryView(operation.category) : null;
	const accessView = operation ? getOperationAccessView(operation.public) : null;

	return (
		<WorkspacePageLayout
			title={operation?.name ?? "Operation details"}
			description="Server route metadata and runtime configuration"
			kicker={
				<Breadcrumbs>
					<Anchor onClick={() => navigate("/operations")}>Operations</Anchor>
					<Text>Detail</Text>
				</Breadcrumbs>
			}
			actions={
				<Button variant="subtle" leftSection={<ArrowLeft size={16} />} onClick={() => navigate("/operations")}>
					Back
				</Button>
			}
		>

			{isLoading && (
				<div className={classes.loadingState}>
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading operation...
					</Text>
				</div>
			)}

			{error && (
				<Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
					{error instanceof Error ? error.message : "Failed to load operation"}
				</Alert>
			)}

			{operation && (
				<>
					<SectionPanel
						title="Operation summary"
						description="Server route metadata and runtime ownership"
						view="filled"
						padding="m"
					>
						<div className={classes.summaryStack}>
							<div className={classes.summaryHeader}>
								<div className={classes.summaryIdentity}>
									<div className={classes.titleRow}>
										<CategoryIcon size={20} color="var(--g-color-text-secondary)" />
										<Text as="h2" variant="subheader-3" className={classes.operationTitle}>
											{operation.name}
										</Text>
									</div>
									<Code size="sm">{operation.id}</Code>
								</div>
								<div className={classes.badgeRow}>
									<Badge variant="light" color={categoryView?.color ?? "gray"}>
										{categoryView?.label ?? operation.category}
									</Badge>
									{operation.public ? (
										<Badge color={accessView?.color ?? "primary"} variant="light" leftSection={<LockOpen size={12} />}>
											{accessView?.label}
										</Badge>
									) : (
										<Badge color={accessView?.color ?? "deep"} variant="light" leftSection={<Lock size={12} />}>
											{accessView?.label}
										</Badge>
									)}
								</div>
							</div>

							<Divider />

							<KeyValueList
								items={[
									{
										id: "path",
										label: "Path pattern",
										value: <Code>{operation.path_pattern}</Code>,
									},
									{
										id: "module",
										label: "Module",
										value: <Code>{operation.module}</Code>,
									},
									{
										id: "access",
										label: "Access contract",
										value: accessView?.label,
										caption: accessView?.description,
									},
									...(operation.app
										? [
												{
													id: "app",
													label: "App",
													value: (
														<Anchor onClick={() => navigate(`/apps/${operation.app?.id}`)}>
															{operation.app?.name}
														</Anchor>
													),
												},
											]
										: []),
								]}
							/>

							<div>
								<Text size="sm" fw={500} className={classes.methodsLabel}>
									HTTP Methods
								</Text>
								<div className={classes.methodList}>
									{operation.methods.map((method) => (
										<MethodBadge key={method} method={method} />
									))}
								</div>
							</div>
						</div>
					</SectionPanel>

					<SectionPanel
						title="Settings"
						description="Editable access policy and operator-facing description"
						view="tinted"
						padding="m"
					>
						<div className={classes.settingsStack}>
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
								<div className={classes.formActions}>
									<Button onClick={handleSave} loading={updateMutation.isPending}>
										Save Changes
									</Button>
									<Button variant="subtle" onClick={handleReset}>
										Reset
									</Button>
								</div>
							)}

							{updateMutation.isError && (
								<Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
									{updateMutation.error instanceof Error ? updateMutation.error.message : "Failed to update operation"}
								</Alert>
							)}

							{updateMutation.isSuccess && (
								<Alert color="primary" variant="light">
									Operation updated successfully
								</Alert>
							)}
						</div>
					</SectionPanel>
				</>
			)}
		</WorkspacePageLayout>
	);
}

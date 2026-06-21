import { useParams, useNavigate } from "react-router-dom";
import {
	Text,
	Badge,
	Skeleton,
	Alert,
	EmptyState,
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
import { CircleAlert as CircleExclamation, ArrowLeft, Lock, LockOpen, Server, Code as CodeIcon, Database, Shield, Boxes as Boxes3, Cpu } from "lucide-react";
import {
	getOperationAccessView,
	getOperationCategoryView,
	getOperationMethodView,
} from "@/entities/operation-catalog";
import { useOperation, useUpdateOperation } from "@/shared/api/hooks";
import { useState, useEffect, useId } from "react";
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
		<Badge size="sm" color={methodView.color}>
			{methodView.method}
		</Badge>
	);
}

export function OperationDetailPage() {
	const { id } = useParams<{ id: string }>();
	const navigate = useNavigate();
	const {
		data: operation,
		isLoading,
		isError,
		error,
		refetch,
	} = useOperation(id ?? "");
	const updateMutation = useUpdateOperation();
	const descriptionId = useId();

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
			<Alert
				theme="danger"
				title="Operation ID is required"
				message="No operation identifier was provided in the URL."
			/>
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
					<Anchor
						href="/operations"
						onClick={(event) => {
							event.preventDefault();
							navigate("/operations");
						}}
					>
						Operations
					</Anchor>
					<Text>Detail</Text>
				</Breadcrumbs>
			}
			actions={
				<Button
					variant="subtle"
					leftSection={<ArrowLeft width={16} height={16} aria-hidden="true" />}
					onClick={() => navigate("/operations")}
				>
					Back
				</Button>
			}
		>

			{isLoading && (
				<div className={classes.loadingStack} aria-busy="true">
					<SectionPanel view="filled" padding="m">
						<div className={classes.summaryStack}>
							<Skeleton className={classes.skeletonTitle} />
							<Skeleton className={classes.skeletonLine} />
							<Skeleton className={classes.skeletonLine} />
							<Skeleton className={classes.skeletonLine} />
						</div>
					</SectionPanel>
					<SectionPanel view="tinted" padding="m">
						<div className={classes.settingsStack}>
							<Skeleton className={classes.skeletonLine} />
							<Skeleton className={classes.skeletonBlock} />
						</div>
					</SectionPanel>
				</div>
			)}

			{isError && (
				<EmptyState
					image={<CircleExclamation width={48} height={48} aria-hidden="true" />}
					title="Failed to load operation"
					description={
						error instanceof Error
							? error.message
							: "Something went wrong while loading this operation."
					}
					actions={[
						{
							text: "Retry",
							variant: "filled",
							onClick: () => {
								void refetch();
							},
						},
					]}
				/>
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
										<CategoryIcon
											width={20}
											height={20}
											color="var(--g-color-text-secondary)"
											aria-hidden="true"
										/>
										<Text as="h2" variant="subheader-3" className={classes.operationTitle}>
											{operation.name}
										</Text>
									</div>
									<Code className={classes.idCode}>{operation.id}</Code>
								</div>
								<div className={classes.badgeRow}>
									<Badge color={categoryView?.color ?? "gray"}>
										{categoryView?.label ?? operation.category}
									</Badge>
									{operation.public ? (
										<Badge
											color={accessView?.color ?? "primary"}
											leftSection={<LockOpen width={12} height={12} aria-hidden="true" />}
										>
											{accessView?.label}
										</Badge>
									) : (
										<Badge
											color={accessView?.color ?? "deep"}
											leftSection={<Lock width={12} height={12} aria-hidden="true" />}
										>
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
														<Anchor
															href={`/apps/${operation.app?.id}`}
															onClick={(event) => {
																event.preventDefault();
																navigate(`/apps/${operation.app?.id}`);
															}}
														>
															{operation.app?.name}
														</Anchor>
													),
												},
											]
										: []),
								]}
							/>

							<div>
								<Text variant="body-2" className={classes.methodsLabel}>
									<strong>HTTP Methods</strong>
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
							<div className={classes.field}>
								<Switch
									content="Public Access"
									checked={isPublic}
									onUpdate={setIsPublic}
								/>
								<Text variant="caption-2" color="secondary">
									When enabled, this operation does not require authentication
								</Text>
							</div>

							<div className={classes.field}>
								<label htmlFor={descriptionId}>
									<Text as="span" variant="body-2">
										<strong>Description</strong>
									</Text>
								</label>
								<Textarea
									id={descriptionId}
									placeholder="Describe what this operation does..."
									value={description}
									onUpdate={setDescription}
									minRows={3}
									maxRows={6}
								/>
								<Text variant="caption-2" color="secondary">
									Optional description for this operation
								</Text>
							</div>

							{hasChanges && (
								<div className={classes.formActions}>
									<Button
										variant="filled"
										onClick={handleSave}
										loading={updateMutation.isPending}
									>
										Save Changes
									</Button>
									<Button variant="subtle" onClick={handleReset}>
										Reset
									</Button>
								</div>
							)}

							{updateMutation.isError && (
								<Alert
									theme="danger"
									title="Update failed"
									message={
										updateMutation.error instanceof Error
											? updateMutation.error.message
											: "Failed to update operation"
									}
								/>
							)}

							{updateMutation.isSuccess && (
								<Alert
									theme="success"
									message="Operation updated successfully"
								/>
							)}
						</div>
					</SectionPanel>
				</>
			)}
		</WorkspacePageLayout>
	);
}

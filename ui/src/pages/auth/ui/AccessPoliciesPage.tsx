import { Button, Card, Field, Form, FormSpy, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import { useState, useMemo } from "react";
import {
	Alert,
	Badge,
	Code,
	Collapse,
	Divider,
	EmptyState,
	Modal,
	MultiSelect,
	NumberInput,
	Select,
	Skeleton,
	Switch,
	DataPreview,
	Text,
	Textarea,
	TextInput,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { DropdownMenu } from "@gravity-ui/uikit";
import { Plus, Search as Magnifier, EllipsisVertical, Pencil, Trash2 as TrashBin, ShieldCheck, ChevronDown, ChevronRight, Code as CodeIcon, Check, X as Xmark } from "lucide-react";
import {
	useAccessPolicies,
	useCreateAccessPolicy,
	useUpdateAccessPolicy,
	useDeleteAccessPolicy,
} from "../lib/useAccessPolicies";
import {
	accessPolicyOperations,
	accessPolicyUserTypes,
	getAccessPolicyEngineView,
	getAccessPolicyPriority,
	getAccessPolicyStatusView,
	type AccessPolicyEngineType,
	type AccessPolicyResource,
	type EngineElement,
	type MatcherElement,
} from "@/entities/access-policy";
import { useClients } from "../lib/useClients";
import { getBundleResources, isRecord } from "@/shared/api/guards";
import { useResourceTypes } from "@/shared/api/hooks";
import { PolicyScriptEditor } from "@/shared/monaco/PolicyScriptEditor";
import type { ClientResource } from "@/entities/oauth-client";
import type { ReactNode } from "react";
import classes from "./AccessPoliciesPage.module.css";

/** Wraps a field with an adjacent helper line (Gravity inputs have no `description`). */
function FieldWithHint({ hint, children }: { hint: ReactNode; children: ReactNode }) {
	return (
		<div className={classes.fieldWithHint}>
			{children}
			<Text variant="caption-2" color="secondary">
				{hint}
			</Text>
		</div>
	);
}

export function AccessPoliciesPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingPolicy, setEditingPolicy] = useState<AccessPolicyResource | null>(null);
	const [deleteTarget, setDeleteTarget] = useState<AccessPolicyResource | null>(null);

	const { data, isLoading, isError, error, refetch } = useAccessPolicies({ search: debouncedSearch });
	const deletePolicy = useDeleteAccessPolicy();

	const handleEdit = (policy: AccessPolicyResource) => {
		setEditingPolicy(policy);
		open();
	};

	const handleDeleteConfirm = () => {
		if (deleteTarget?.id) {
			deletePolicy.mutate(deleteTarget.id, {
				onSuccess: () => setDeleteTarget(null),
			});
		}
	};

	const handleClose = () => {
		setEditingPolicy(null);
		close();
	};

	const policies = getBundleResources<AccessPolicyResource>(data);
	const isFiltered = debouncedSearch.length > 0;

	return (
		<WorkspacePageLayout
			title="Access Policies"
			description="Define fine-grained access control rules with matchers and custom scripts"
			actions={
				<Button view="action" onClick={open}>
					<Button.Icon>
						<Plus width={16} />
					</Button.Icon>
					Create Policy
				</Button>
			}
			toolbar={
				<div className={classes.toolbar}>
					<TextInput
						aria-label="Search policies by name"
						placeholder="Search by name..."
						leftSection={<Magnifier width={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						className={classes.search}
					/>
				</div>
			}
		>

			<Card className={classes.tableContainer}>
				{isLoading ? (
					<div className={classes.skeletonList}>
						{["a", "b", "c", "d", "e"].map((k) => (
							<Skeleton key={k} className={classes.skeletonRow} />
						))}
					</div>
				) : isError ? (
					<EmptyState
						title="Failed to load policies"
						description={error instanceof Error ? error.message : "Something went wrong while loading access policies."}
						actions={[
							<Button key="retry" view="action" onClick={() => refetch()}>
								Retry
							</Button>,
						]}
					/>
				) : policies.length === 0 ? (
					<EmptyState
						title={isFiltered ? "No matching policies" : "No access policies yet"}
						description={
							isFiltered
								? "No policies match your search. Try a different term."
								: "Define fine-grained access control rules with matchers and custom scripts."
						}
						actions={
							isFiltered
								? [
										<Button key="clear" view="outlined" onClick={() => setSearch("")}>
											Clear filters
										</Button>,
									]
								: [
										<Button key="create" view="action" onClick={open}>
											Create Policy
										</Button>,
									]
						}
					/>
				) : (
					<DataPreview
						columns={[
							{ id: "name", label: "Name" },
							{ id: "engine", label: "Engine", width: 150 },
							{ id: "priority", label: "Priority", width: 96 },
							{ id: "status", label: "Status", width: 110 },
							{ id: "actions", label: "", width: 48 },
						]}
						rows={policies.map((policy) => {
							const statusView = getAccessPolicyStatusView(policy);

							return {
								id: policy.id ?? policy.name,
								name: (
									<div className={classes.policyCell}>
										<ShieldCheck width={16} height={16} className={classes.policyIcon} aria-hidden="true" />
										<div className={classes.policyText}>
											<Text variant="body-2" className={classes.policyName}>
												<strong>{policy.name}</strong>
											</Text>
											<Text variant="caption-2" color="secondary" className={classes.policyDescription}>
												{policy.description || "No description"}
											</Text>
										</div>
									</div>
								),
								engine: <EngineTypeBadge type={policy.engine?.type} />,
								priority: <Text variant="body-2">{getAccessPolicyPriority(policy)}</Text>,
								status: <Badge color={statusView.color}>{statusView.label}</Badge>,
								actions: (
									<DropdownMenu
										size="s"
										icon={<EllipsisVertical width={16} />}
										defaultSwitcherProps={{
											view: "flat-secondary",
											size: "s",
											"aria-label": "Policy actions",
											"aria-haspopup": "menu",
										}}
										popupProps={{ placement: "bottom-end" }}
										items={[
											{
												text: "Edit",
												iconStart: <Pencil width={14} />,
												action: () => handleEdit(policy),
											},
											[
												{
													text: "Delete",
													iconStart: <TrashBin width={14} />,
													theme: "danger",
													action: () => setDeleteTarget(policy),
												},
											],
										]}
									/>
								),
							};
						})}
						getRowKey={(row, index) => String(row.id ?? policies[index]?.id ?? index)}
					/>
				)}
			</Card>

			<PolicyModal opened={opened} onClose={handleClose} policy={editingPolicy} />

			<DeletePolicyModal
				opened={!!deleteTarget}
				onClose={() => setDeleteTarget(null)}
				onConfirm={handleDeleteConfirm}
				policyName={deleteTarget?.name ?? ""}
				isDeleting={deletePolicy.isPending}
			/>
		</WorkspacePageLayout>
	);
}

function EngineTypeBadge({ type }: { type?: AccessPolicyEngineType }) {
	const view = getAccessPolicyEngineView(type);

	switch (type) {
		case "allow":
			return (
				<Badge color={view.color} leftSection={<Check width={12} height={12} aria-hidden="true" />}>
					{view.label}
				</Badge>
			);
		case "deny":
			return (
				<Badge color={view.color} leftSection={<Xmark width={12} height={12} aria-hidden="true" />}>
					{view.label}
				</Badge>
			);
		case "quickjs":
			return (
				<Badge color={view.color} leftSection={<CodeIcon width={12} height={12} aria-hidden="true" />}>
					{view.label}
				</Badge>
			);
		default:
			return <Badge color={view.color}>{view.label}</Badge>;
	}
}

function DeletePolicyModal({
	opened,
	onClose,
	onConfirm,
	policyName,
	isDeleting,
}: {
	opened: boolean;
	onClose: () => void;
	onConfirm: () => void;
	policyName: string;
	isDeleting: boolean;
}) {
	return (
		<Modal opened={opened} onClose={onClose} title="Delete Access Policy" size="md">
			<div className={classes.deleteModalContent}>
				<Text variant="body-2">
					You are about to delete the policy: <strong>{policyName}</strong>
				</Text>

				<Alert
					theme="danger"
					title="This action cannot be undone."
					message="Requests that relied on this policy will fall back to other matching rules."
				/>

				<div className={classes.formActions}>
					<Button view="flat-secondary" onClick={onClose} disabled={isDeleting}>
						Cancel
					</Button>
					<Button view="flat-danger" onClick={onConfirm} loading={isDeleting}>
						Delete Policy
					</Button>
				</div>
			</div>
		</Modal>
	);
}

interface PolicyFormValues {
	name: string;
	description: string;
	active: boolean;
	priority: number;
	engineType: "allow" | "deny" | "quickjs";
	script: string;
	denyMessage: string;
	// Matcher fields
	clients: string[];
	roles: string[];
	userTypes: string[];
	resourceTypes: string[];
	operations: string[];
	operationIds: string[];
	paths: string[];
	sourceIps: string[];
}

const POLICY_DEFAULTS: PolicyFormValues = {
	name: "",
	description: "",
	active: true,
	priority: 100,
	engineType: "allow",
	script: "",
	denyMessage: "",
	clients: [],
	roles: [],
	userTypes: [],
	resourceTypes: [],
	operations: [],
	operationIds: [],
	paths: [],
	sourceIps: [],
};

function validatePolicy(values: PolicyFormValues) {
	const errors: Partial<Record<keyof PolicyFormValues, string>> = {};
	if (!values.name || values.name.length < 3) errors.name = "Name must be at least 3 characters";
	if (values.engineType === "quickjs" && !values.script.trim())
		errors.script = "Script is required for QuickJS engine";
	if (values.priority < 0 || values.priority > 1000)
		errors.priority = "Priority must be between 0 and 1000";
	return errors;
}

function PolicyModal({
	opened,
	onClose,
	policy,
}: {
	opened: boolean;
	onClose: () => void;
	policy: AccessPolicyResource | null;
}) {
	const create = useCreateAccessPolicy();
	const update = useUpdateAccessPolicy();
	const { data: clientsData } = useClients({ count: 100 });
	const { data: resourceTypes } = useResourceTypes();
	const [matcherOpen, matcherHandlers] = useDisclosure(false);

	const isEditing = !!policy;

	const initialValues: PolicyFormValues = policy
		? {
				name: policy.name,
				description: policy.description ?? "",
				active: policy.active !== false,
				priority: policy.priority ?? 100,
				engineType: policy.engine?.type ?? "allow",
				script: policy.engine?.script ?? "",
				denyMessage: policy.denyMessage ?? "",
				clients: policy.matcher?.clients ?? [],
				roles: policy.matcher?.roles ?? [],
				userTypes: policy.matcher?.userTypes ?? [],
				resourceTypes: policy.matcher?.resourceTypes ?? [],
				operations: policy.matcher?.operations ?? [],
				operationIds: policy.matcher?.operationIds ?? [],
				paths: policy.matcher?.paths ?? [],
				sourceIps: policy.matcher?.sourceIps ?? [],
			}
		: POLICY_DEFAULTS;

	const baseClientOptions = useMemo(() => {
		const serverClients = getBundleResources(clientsData, isClientResource).map((client) => ({
			label: client.name,
			value: client.clientId,
		}));
		const seen = new Set<string>();
		return serverClients.filter((c) => {
			if (seen.has(c.value)) return false;
			seen.add(c.value);
			return true;
		});
	}, [clientsData]);

	const typeOptions = useMemo(() => ["*", ...(resourceTypes ?? [])], [resourceTypes]);
	const operationOptions = accessPolicyOperations.map((op) => ({ label: op, value: op }));
	const userTypeOptions = accessPolicyUserTypes.map((ut) => ({ label: ut, value: ut }));

	const handleSubmit = async (values: PolicyFormValues) => {
		const matcherEl: MatcherElement = {};
		if (values.clients.length > 0) matcherEl.clients = values.clients;
		if (values.roles.length > 0) matcherEl.roles = values.roles;
		if (values.userTypes.length > 0) matcherEl.userTypes = values.userTypes;
		if (values.resourceTypes.length > 0) matcherEl.resourceTypes = values.resourceTypes;
		if (values.operations.length > 0) matcherEl.operations = values.operations;
		if (values.operationIds.length > 0) matcherEl.operationIds = values.operationIds;
		if (values.paths.length > 0) matcherEl.paths = values.paths;
		if (values.sourceIps.length > 0) matcherEl.sourceIps = values.sourceIps;

		const engine: EngineElement = { type: values.engineType };
		if (values.engineType === "quickjs") engine.script = values.script;

		const payload: Partial<AccessPolicyResource> = {
			resourceType: "AccessPolicy",
			name: values.name,
			description: values.description || undefined,
			active: values.active,
			priority: values.priority,
			engine,
			denyMessage: values.denyMessage || undefined,
		};
		if (Object.keys(matcherEl).length > 0) payload.matcher = matcherEl;

		try {
			if (isEditing && policy?.id) {
				await update.mutateAsync({
					...policy,
					...payload,
					id: policy.id,
					resourceType: "AccessPolicy",
					engine,
				});
			} else {
				await create.mutateAsync(payload);
			}
			onClose();
		} catch {
			/* surfaced by mutation */
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit Access Policy" : "Create Access Policy"}
			size="xl"
		>
			<Form<PolicyFormValues>
				key={policy?.id ?? "new"}
				onSubmit={handleSubmit}
				validate={validatePolicy}
				initialValues={initialValues}
				render={({ handleSubmit: submit, values, form: api, submitting }) => {
					const hasMatcherValues =
						values.clients.length > 0 ||
						values.roles.length > 0 ||
						values.userTypes.length > 0 ||
						values.resourceTypes.length > 0 ||
						values.operations.length > 0 ||
						values.operationIds.length > 0 ||
						values.paths.length > 0 ||
						values.sourceIps.length > 0;

					const customClients = values.clients
						.filter((c) => !baseClientOptions.some((o) => o.value === c))
						.map((c) => ({ label: c, value: c }));
					const clientOptions = [...baseClientOptions, ...customClients];

					return (
						<form onSubmit={submit}>
							<div className={classes.policyForm}>
								<div className={classes.formGrid}>
									<Field<string> name="name">
										{({ input, meta }) => (
											<TextInput
												label="Policy Name"
												required
												value={input.value}
												onChange={input.onChange}
												onBlur={input.onBlur}
												error={meta.touched && meta.error ? meta.error : undefined}
											/>
										)}
									</Field>
									<Field<number> name="priority">
										{({ input, meta }) => (
											<FieldWithHint hint="Lower = evaluated first (0-1000)">
												<NumberInput
													label="Priority"
													min={0}
													max={1000}
													value={input.value}
													onChange={input.onChange}
													error={meta.touched && meta.error ? meta.error : undefined}
												/>
											</FieldWithHint>
										)}
									</Field>
								</div>

								<Field<string> name="description">
									{({ input }) => (
										<Textarea
											label="Description"
											placeholder="What does this policy do?"
											value={input.value}
											onChange={input.onChange}
										/>
									)}
								</Field>

								<Field<boolean> name="active" type="checkbox">
									{({ input }) => (
										<div className={classes.switchField}>
											<Switch
												content="Active"
												checked={input.checked ?? false}
												onUpdate={input.onChange}
											/>
											<Text variant="caption-2" color="secondary">
												Inactive policies are not evaluated
											</Text>
										</div>
									)}
								</Field>

								<Divider align="center">Engine</Divider>

								<Field<string> name="engineType">
									{({ input }) => (
										<FieldWithHint hint="How access decisions are made">
											<Select
												label="Engine Type"
												data={[
													{ label: "Allow - Always allow access", value: "allow" },
													{ label: "Deny - Always deny access", value: "deny" },
													{ label: "QuickJS - Custom JavaScript policy", value: "quickjs" },
												]}
												value={input.value}
												onChange={input.onChange}
											/>
										</FieldWithHint>
									)}
								</Field>

								{values.engineType === "quickjs" && (
									<div className={classes.scriptSection}>
										<Text variant="body-2" className={classes.scriptLabel}>
											<strong>Policy Script</strong>{" "}
											<span className={classes.requiredMark}>*</span>
										</Text>
										<Text variant="caption-2" color="secondary" className={classes.scriptHint}>
											Write JavaScript to evaluate access. Use <Code>allow()</Code>,{" "}
											<Code>deny(reason)</Code>, <Code>abstain()</Code>. Press Ctrl+Space for autocomplete.
										</Text>
										<div className={classes.editorFrame}>
											<PolicyScriptEditor
												value={values.script}
												onChange={(val) => api.change("script", val)}
												height={200}
											/>
										</div>
										<FormSpy<PolicyFormValues> subscription={{ errors: true, touched: true }}>
											{({ errors, touched }) => {
												const scriptError =
													typeof errors?.script === "string" ? errors.script : undefined;
												return scriptError && touched?.script ? (
													<Text variant="caption-2" color="danger" className={classes.scriptError}>
														{scriptError}
													</Text>
												) : null;
											}}
										</FormSpy>
									</div>
								)}

								{(values.engineType === "deny" || values.engineType === "quickjs") && (
									<Field<string> name="denyMessage">
										{({ input }) => (
											<TextInput
												label="Deny Message"
												placeholder="Custom message when access is denied"
												value={input.value}
												onChange={input.onChange}
											/>
										)}
									</Field>
								)}

								<Divider align="center">Matcher</Divider>

								<button
									type="button"
									className={classes.matcherToggle}
									onClick={matcherHandlers.toggle}
									aria-expanded={matcherOpen}
								>
									{matcherOpen ? (
										<ChevronDown width={16} aria-hidden="true" />
									) : (
										<ChevronRight width={16} aria-hidden="true" />
									)}
									<span>Matcher</span>
									{hasMatcherValues && <Badge size="sm">Configured</Badge>}
								</button>

								<Text variant="caption-2" color="secondary">
									Define when this policy applies. All specified conditions must match (AND logic).
									Leave empty to match all requests.
								</Text>

								<Collapse in={matcherOpen}>
									<div className={classes.matcherFields}>
										<div className={classes.formGrid}>
											<Field<string[]> name="roles">
												{({ input }) => (
													<FieldWithHint hint="User must have any of these roles">
														<MultiSelect
															label="Roles"
															placeholder="e.g. admin, practitioner"
															data={[...new Set(["admin", "practitioner", "patient", "nurse", ...input.value])]}
															searchable
															value={input.value}
															onChange={input.onChange}
														/>
													</FieldWithHint>
												)}
											</Field>
											<Field<string[]> name="clients">
												{({ input }) => (
													<FieldWithHint hint="OAuth client IDs (supports * wildcard)">
														<MultiSelect
															label="Clients"
															placeholder="Select or add clients"
															data={clientOptions}
															searchable
															value={input.value}
															onChange={input.onChange}
														/>
													</FieldWithHint>
												)}
											</Field>
										</div>

										<div className={classes.formGrid}>
											<Field<string[]> name="userTypes">
												{({ input }) => (
													<FieldWithHint hint="User's FHIR resource type">
														<MultiSelect
															label="User Types"
															data={userTypeOptions}
															searchable
															value={input.value}
															onChange={input.onChange}
														/>
													</FieldWithHint>
												)}
											</Field>
											<Field<string[]> name="resourceTypes">
												{({ input }) => (
													<FieldWithHint hint="Target FHIR resource types">
														<MultiSelect
															label="Resource Types"
															data={typeOptions}
															searchable
															value={input.value}
															onChange={input.onChange}
														/>
													</FieldWithHint>
												)}
											</Field>
										</div>

										<Field<string[]> name="operations">
											{({ input }) => (
												<FieldWithHint hint="FHIR operations to match">
													<MultiSelect
														label="Operations"
														data={operationOptions}
														searchable
														value={input.value}
														onChange={input.onChange}
													/>
												</FieldWithHint>
											)}
										</Field>

										<div className={classes.formGrid}>
											<Field<string[]> name="operationIds">
												{({ input }) => (
													<FieldWithHint hint="Specific operation IDs (e.g. fhir.read, graphql.query)">
														<MultiSelect
															label="Operation IDs"
															placeholder="Add operation ID"
															data={input.value}
															searchable
															value={input.value}
															onChange={input.onChange}
														/>
													</FieldWithHint>
												)}
											</Field>
											<Field<string[]> name="paths">
												{({ input }) => (
													<FieldWithHint hint="Request path patterns (glob syntax)">
														<MultiSelect
															label="Paths"
															placeholder="e.g. /Patient/*, /admin/*"
															data={input.value}
															searchable
															value={input.value}
															onChange={input.onChange}
														/>
													</FieldWithHint>
												)}
											</Field>
										</div>

										<Field<string[]> name="sourceIps">
											{({ input }) => (
												<FieldWithHint hint="Client IP addresses in CIDR notation">
													<MultiSelect
														label="Source IPs"
														placeholder="e.g. 192.168.1.0/24, 10.0.0.0/8"
														data={input.value}
														searchable
														value={input.value}
														onChange={input.onChange}
													/>
												</FieldWithHint>
											)}
										</Field>
									</div>
								</Collapse>

								<div className={classes.formActions}>
									<Button view="flat-secondary" onClick={onClose} type="button">
										Cancel
									</Button>
									<Button
										view="action"
										type="submit"
										loading={submitting || create.isPending || update.isPending}
									>
										{isEditing ? "Update" : "Create"}
									</Button>
								</div>
							</div>
						</form>
					);
				}}
			/>
		</Modal>
	);
}

function isClientResource(value: unknown): value is ClientResource {
	return (
		isRecord(value) &&
		value.resourceType === "Client" &&
		typeof value.name === "string" &&
		typeof value.clientId === "string"
	);
}

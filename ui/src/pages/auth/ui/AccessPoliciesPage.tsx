import { useState, useMemo, useEffect } from "react";
import {
	ActionIcon,
	Badge,
	Box,
	Code,
	Collapse,
	Divider,
	Group,
	Menu,
	Modal,
	MultiSelect,
	NumberInput,
	Paper,
	Select,
	Stack,
	Switch,
	Table,
	Text,
	Textarea,
	TextInput,
	Title,
} from "@/shared/ui";
import { Field, Form, FormSpy, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import {
	Plus,
	Magnifier,
	EllipsisVertical,
	Pencil,
	TrashBin,
	ShieldCheck,
	ChevronDown,
	ChevronRight,
	Code as CodeIcon,
	Check,
	Xmark,
} from "@gravity-ui/icons";
import {
	useAccessPolicies,
	useCreateAccessPolicy,
	useUpdateAccessPolicy,
	useDeleteAccessPolicy,
	type AccessPolicyResource,
	type MatcherElement,
	type EngineElement,
	VALID_OPERATIONS,
	VALID_USER_TYPES,
} from "../lib/useAccessPolicies";
import { useClients } from "../lib/useClients";
import { useResourceTypes } from "@/shared/api/hooks";
import { Button } from "@/shared/ui/Button/Button";
import { PolicyScriptEditor } from "@/shared/monaco/PolicyScriptEditor";

export function AccessPoliciesPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingPolicy, setEditingPolicy] = useState<AccessPolicyResource | null>(null);

	const { data, isLoading } = useAccessPolicies({ search: debouncedSearch });
	const deletePolicy = useDeleteAccessPolicy();

	const handleEdit = (policy: AccessPolicyResource) => {
		setEditingPolicy(policy);
		open();
	};

	const handleDelete = (id: string) => {
		if (confirm("Are you sure you want to delete this policy?")) {
			deletePolicy.mutate(id);
		}
	};

	const handleClose = () => {
		setEditingPolicy(null);
		close();
	};

	const policies = data?.entry?.map((e) => e.resource) || [];

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<Group justify="space-between">
				<div>
					<Title order={2}>Access Policies</Title>
					<Text c="dimmed" size="sm">
						Define fine-grained access control rules with matchers and custom scripts
					</Text>
				</div>
				<Button leftSection={<Plus size={16} />} onClick={open}>
					Create Policy
				</Button>
			</Group>

			<Paper p="md" withBorder>
				<Group mb="md">
					<TextInput
						placeholder="Search by name..."
						leftSection={<Magnifier size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						style={{ flex: 1 }}
					/>
				</Group>

				<Table>
					<Table.Thead>
						<Table.Tr>
							<Table.Th>Name</Table.Th>
							<Table.Th>Engine</Table.Th>
							<Table.Th>Priority</Table.Th>
							<Table.Th>Status</Table.Th>
							<Table.Th style={{ width: 50 }} />
						</Table.Tr>
					</Table.Thead>
					<Table.Tbody>
						{isLoading ? (
							<Table.Tr>
								<Table.Td colSpan={5}>Loading...</Table.Td>
							</Table.Tr>
						) : policies.length === 0 ? (
							<Table.Tr>
								<Table.Td colSpan={5} style={{ textAlign: "center" }}>
									No policies found
								</Table.Td>
							</Table.Tr>
						) : (
							policies.map((policy) => (
								<Table.Tr key={policy.id}>
									<Table.Td>
										<Group gap="xs">
											<ShieldCheck size={16} color="green" />
											<div>
												<Text size="sm" fw={500}>
													{policy.name}
												</Text>
												<Text size="xs" c="dimmed">
													{policy.description || "No description"}
												</Text>
											</div>
										</Group>
									</Table.Td>
									<Table.Td>
										<EngineTypeBadge type={policy.engine?.type} />
									</Table.Td>
									<Table.Td>
										<Text size="sm">{policy.priority ?? 100}</Text>
									</Table.Td>
									<Table.Td>
										<Badge
											color={policy.active !== false ? "green" : "gray"}
											variant="light"
										>
											{policy.active !== false ? "Active" : "Inactive"}
										</Badge>
									</Table.Td>
									<Table.Td>
										<Menu position="bottom-end" withinPortal>
											<Menu.Target>
												<ActionIcon variant="subtle" color="gray">
													<EllipsisVertical size={16} />
												</ActionIcon>
											</Menu.Target>
											<Menu.Dropdown>
												<Menu.Item
													leftSection={<Pencil size={14} />}
													onClick={() => handleEdit(policy)}
												>
													Edit
												</Menu.Item>
												<Menu.Item
													leftSection={<TrashBin size={14} />}
													color="red"
													onClick={() => policy.id && handleDelete(policy.id)}
												>
													Delete
												</Menu.Item>
											</Menu.Dropdown>
										</Menu>
									</Table.Td>
								</Table.Tr>
							))
						)}
					</Table.Tbody>
				</Table>
			</Paper>

			<PolicyModal opened={opened} onClose={handleClose} policy={editingPolicy} />
		</Stack>
	);
}

function EngineTypeBadge({ type }: { type?: string }) {
	switch (type) {
		case "allow":
			return (
				<Badge color="green" variant="light" leftSection={<Check size={12} />}>
					Allow
				</Badge>
			);
		case "deny":
			return (
				<Badge color="red" variant="light" leftSection={<Xmark size={12} />}>
					Deny
				</Badge>
			);
		case "quickjs":
			return (
				<Badge color="blue" variant="light" leftSection={<CodeIcon width={12} />}>
					QuickJS Script
				</Badge>
			);
		default:
			return <Badge color="gray">Unknown</Badge>;
	}
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
	const matcher = useDisclosure(false);

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
		const serverClients =
			clientsData?.entry?.map((e) => ({
				label: e.resource.name,
				value: e.resource.clientId,
			})) || [];
		const seen = new Set<string>();
		return serverClients.filter((c) => {
			if (seen.has(c.value)) return false;
			seen.add(c.value);
			return true;
		});
	}, [clientsData]);

	const typeOptions = useMemo(() => ["*", ...(resourceTypes ?? [])], [resourceTypes]);
	const operationOptions = VALID_OPERATIONS.map((op) => ({ label: op, value: op }));
	const userTypeOptions = VALID_USER_TYPES.map((ut) => ({ label: ut, value: ut }));

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
				await update.mutateAsync({ ...payload, id: policy.id } as AccessPolicyResource);
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
							<Stack gap="md">
								<Group grow>
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
											<NumberInput
												label="Priority"
												description="Lower = evaluated first (0-1000)"
												min={0}
												max={1000}
												value={input.value}
												onChange={input.onChange}
												error={meta.touched && meta.error ? meta.error : undefined}
											/>
										)}
									</Field>
								</Group>

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
										<Switch
											label="Active"
											description="Inactive policies are not evaluated"
											checked={input.checked ?? false}
											onChange={input.onChange}
										/>
									)}
								</Field>

								<Divider label="Engine" labelPosition="center" />

								<Field<string> name="engineType">
									{({ input }) => (
										<Select
											label="Engine Type"
											description="How access decisions are made"
											data={[
												{ label: "Allow - Always allow access", value: "allow" },
												{ label: "Deny - Always deny access", value: "deny" },
												{ label: "QuickJS - Custom JavaScript policy", value: "quickjs" },
											]}
											value={input.value}
											onChange={input.onChange}
										/>
									)}
								</Field>

								{values.engineType === "quickjs" && (
									<Box>
										<Text size="sm" fw={500} mb={4}>
											Policy Script{" "}
											<span style={{ color: "var(--g-color-base-danger-medium)" }}>*</span>
										</Text>
										<Text size="xs" c="dimmed" mb="xs">
											Write JavaScript to evaluate access. Use <Code>allow()</Code>,{" "}
											<Code>deny(reason)</Code>, <Code>abstain()</Code>. Press Ctrl+Space for autocomplete.
										</Text>
										<Paper withBorder style={{ overflow: "hidden", borderRadius: 8 }}>
											<PolicyScriptEditor
												value={values.script}
												onChange={(val) => api.change("script", val)}
												height={200}
											/>
										</Paper>
										<FormSpy<PolicyFormValues> subscription={{ errors: true, touched: true }}>
											{({ errors, touched }) =>
												errors?.script && touched?.script ? (
													<Text size="xs" c="red" mt={4}>
														{errors.script as string}
													</Text>
												) : null
											}
										</FormSpy>
									</Box>
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

								<Divider
									label={
										<Group gap="xs" style={{ cursor: "pointer" }} onClick={matcher.toggle}>
											{matcher.isOpen ? <ChevronDown width={16} /> : <ChevronRight width={16} />}
											<span>
												Matcher{" "}
												{hasMatcherValues && (
													<Badge size="xs" variant="light" ml={4}>
														Configured
													</Badge>
												)}
											</span>
										</Group>
									}
									labelPosition="center"
								/>

								<Text size="xs" c="dimmed">
									Define when this policy applies. All specified conditions must match (AND logic).
									Leave empty to match all requests.
								</Text>

								<Collapse in={matcher.isOpen}>
									<Stack gap="md" pt="xs">
										<Group grow align="flex-start">
											<Field<string[]> name="roles">
												{({ input }) => (
													<MultiSelect
														label="Roles"
														description="User must have any of these roles"
														placeholder="e.g. admin, practitioner"
														data={[...new Set(["admin", "practitioner", "patient", "nurse", ...input.value])]}
														searchable
														value={input.value}
														onChange={input.onChange}
													/>
												)}
											</Field>
											<Field<string[]> name="clients">
												{({ input }) => (
													<MultiSelect
														label="Clients"
														description="OAuth client IDs (supports * wildcard)"
														placeholder="Select or add clients"
														data={clientOptions}
														searchable
														value={input.value}
														onChange={input.onChange}
													/>
												)}
											</Field>
										</Group>

										<Group grow align="flex-start">
											<Field<string[]> name="userTypes">
												{({ input }) => (
													<MultiSelect
														label="User Types"
														description="User's FHIR resource type"
														data={userTypeOptions}
														searchable
														value={input.value}
														onChange={input.onChange}
													/>
												)}
											</Field>
											<Field<string[]> name="resourceTypes">
												{({ input }) => (
													<MultiSelect
														label="Resource Types"
														description="Target FHIR resource types"
														data={typeOptions}
														searchable
														value={input.value}
														onChange={input.onChange}
													/>
												)}
											</Field>
										</Group>

										<Field<string[]> name="operations">
											{({ input }) => (
												<MultiSelect
													label="Operations"
													description="FHIR operations to match"
													data={operationOptions}
													searchable
													value={input.value}
													onChange={input.onChange}
												/>
											)}
										</Field>

										<Group grow align="flex-start">
											<Field<string[]> name="operationIds">
												{({ input }) => (
													<MultiSelect
														label="Operation IDs"
														description="Specific operation IDs (e.g. fhir.read, graphql.query)"
														placeholder="Add operation ID"
														data={input.value}
														searchable
														value={input.value}
														onChange={input.onChange}
													/>
												)}
											</Field>
											<Field<string[]> name="paths">
												{({ input }) => (
													<MultiSelect
														label="Paths"
														description="Request path patterns (glob syntax)"
														placeholder="e.g. /Patient/*, /admin/*"
														data={input.value}
														searchable
														value={input.value}
														onChange={input.onChange}
													/>
												)}
											</Field>
										</Group>

										<Field<string[]> name="sourceIps">
											{({ input }) => (
												<MultiSelect
													label="Source IPs"
													description="Client IP addresses in CIDR notation"
													placeholder="e.g. 192.168.1.0/24, 10.0.0.0/8"
													data={input.value}
													searchable
													value={input.value}
													onChange={input.onChange}
												/>
											)}
										</Field>
									</Stack>
								</Collapse>

								<Group justify="flex-end" mt="md">
									<Button variant="light" onClick={onClose} type="button">
										Cancel
									</Button>
									<Button
										type="submit"
										loading={submitting || create.isPending || update.isPending}
									>
										{isEditing ? "Update" : "Create"}
									</Button>
								</Group>
							</Stack>
						</form>
					);
				}}
			/>
		</Modal>
	);
}

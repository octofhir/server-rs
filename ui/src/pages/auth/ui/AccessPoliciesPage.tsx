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
import { useDisclosure, useDebouncedValue } from "@octofhir/ui-kit";
import { useForm } from "@mantine/form";
import {
	IconPlus,
	IconSearch,
	IconDotsVertical,
	IconEdit,
	IconTrash,
	IconShieldCheck,
	IconChevronDown,
	IconChevronRight,
	IconCode,
	IconCheck,
	IconX,
} from "@tabler/icons-react";
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
				<Button leftSection={<IconPlus size={16} />} onClick={open}>
					Create Policy
				</Button>
			</Group>

			<Paper p="md" withBorder>
				<Group mb="md">
					<TextInput
						placeholder="Search by name..."
						leftSection={<IconSearch size={16} />}
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
											<IconShieldCheck size={16} color="green" />
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
													<IconDotsVertical size={16} />
												</ActionIcon>
											</Menu.Target>
											<Menu.Dropdown>
												<Menu.Item
													leftSection={<IconEdit size={14} />}
													onClick={() => handleEdit(policy)}
												>
													Edit
												</Menu.Item>
												<Menu.Item
													leftSection={<IconTrash size={14} />}
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
				<Badge color="green" variant="light" leftSection={<IconCheck size={12} />}>
					Allow
				</Badge>
			);
		case "deny":
			return (
				<Badge color="red" variant="light" leftSection={<IconX size={12} />}>
					Deny
				</Badge>
			);
		case "quickjs":
			return (
				<Badge color="blue" variant="light" leftSection={<IconCode size={12} />}>
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
	const [matcherExpanded, { toggle: toggleMatcher }] = useDisclosure(false);

	const isEditing = !!policy;

	const form = useForm<PolicyFormValues>({
		initialValues: {
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
		},
		validate: {
			name: (value) => (value.length < 3 ? "Name must be at least 3 characters" : null),
			script: (value, values) =>
				values.engineType === "quickjs" && !value.trim()
					? "Script is required for QuickJS engine"
					: null,
			priority: (value) =>
				value < 0 || value > 1000 ? "Priority must be between 0 and 1000" : null,
		},
	});

	// Reset form when policy changes
	// biome-ignore lint/correctness/useExhaustiveDependencies: form methods are stable
	useEffect(() => {
		if (policy) {
			form.setValues({
				name: policy.name,
				description: policy.description || "",
				active: policy.active !== false,
				priority: policy.priority ?? 100,
				engineType: policy.engine?.type || "allow",
				script: policy.engine?.script || "",
				denyMessage: policy.denyMessage || "",
				clients: policy.matcher?.clients || [],
				roles: policy.matcher?.roles || [],
				userTypes: policy.matcher?.userTypes || [],
				resourceTypes: policy.matcher?.resourceTypes || [],
				operations: policy.matcher?.operations || [],
				operationIds: policy.matcher?.operationIds || [],
				paths: policy.matcher?.paths || [],
				sourceIps: policy.matcher?.sourceIps || [],
			});
		} else {
			form.reset();
		}
	}, [policy]);

	const clientOptions = useMemo(() => {
		const serverClients =
			clientsData?.entry?.map((e) => ({
				label: e.resource.name,
				value: e.resource.clientId,
			})) || [];
		// Dedupe server clients (in case of duplicates in DB)
		const seen = new Set<string>();
		const uniqueServerClients = serverClients.filter((c) => {
			if (seen.has(c.value)) return false;
			seen.add(c.value);
			return true;
		});
		// Merge with form values to avoid duplicates when editing
		const customClients = form.values.clients
			.filter((c) => !seen.has(c))
			.map((c) => ({ label: c, value: c }));
		return [...uniqueServerClients, ...customClients];
	}, [clientsData, form.values.clients]);

	const typeOptions = useMemo(() => {
		const types = resourceTypes || [];
		return ["*", ...types];
	}, [resourceTypes]);

	const operationOptions = VALID_OPERATIONS.map((op) => ({ label: op, value: op }));
	const userTypeOptions = VALID_USER_TYPES.map((ut) => ({ label: ut, value: ut }));

	const handleSubmit = async (values: PolicyFormValues) => {
		// Build matcher object (only include non-empty arrays)
		const matcher: MatcherElement = {};
		if (values.clients.length > 0) matcher.clients = values.clients;
		if (values.roles.length > 0) matcher.roles = values.roles;
		if (values.userTypes.length > 0) matcher.userTypes = values.userTypes;
		if (values.resourceTypes.length > 0) matcher.resourceTypes = values.resourceTypes;
		if (values.operations.length > 0) matcher.operations = values.operations;
		if (values.operationIds.length > 0) matcher.operationIds = values.operationIds;
		if (values.paths.length > 0) matcher.paths = values.paths;
		if (values.sourceIps.length > 0) matcher.sourceIps = values.sourceIps;

		// Build engine object
		const engine: EngineElement = {
			type: values.engineType,
		};
		if (values.engineType === "quickjs") {
			engine.script = values.script;
		}

		const payload: Partial<AccessPolicyResource> = {
			resourceType: "AccessPolicy",
			name: values.name,
			description: values.description || undefined,
			active: values.active,
			priority: values.priority,
			engine,
			denyMessage: values.denyMessage || undefined,
		};

		// Only include matcher if it has any fields
		if (Object.keys(matcher).length > 0) {
			payload.matcher = matcher;
		}

		try {
			if (isEditing && policy?.id) {
				await update.mutateAsync({ ...payload, id: policy.id } as AccessPolicyResource);
			} else {
				await create.mutateAsync(payload);
			}
			onClose();
		} catch {
			// Handled by hook
		}
	};

	const hasMatcherValues =
		form.values.clients.length > 0 ||
		form.values.roles.length > 0 ||
		form.values.userTypes.length > 0 ||
		form.values.resourceTypes.length > 0 ||
		form.values.operations.length > 0 ||
		form.values.operationIds.length > 0 ||
		form.values.paths.length > 0 ||
		form.values.sourceIps.length > 0;

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit Access Policy" : "Create Access Policy"}
			size="xl"
		>
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					{/* Basic Info */}
					<Group grow>
						<TextInput label="Policy Name" required {...form.getInputProps("name")} />
						<NumberInput
							label="Priority"
							description="Lower = evaluated first (0-1000)"
							min={0}
							max={1000}
							{...form.getInputProps("priority")}
						/>
					</Group>

					<Textarea
						label="Description"
						placeholder="What does this policy do?"
						{...form.getInputProps("description")}
					/>

					<Switch
						label="Active"
						description="Inactive policies are not evaluated"
						{...form.getInputProps("active", { type: "checkbox" })}
					/>

					<Divider label="Engine" labelPosition="center" />

					{/* Engine Configuration */}
					<Select
						label="Engine Type"
						description="How access decisions are made"
						data={[
							{ label: "Allow - Always allow access", value: "allow" },
							{ label: "Deny - Always deny access", value: "deny" },
							{ label: "QuickJS - Custom JavaScript policy", value: "quickjs" },
						]}
						{...form.getInputProps("engineType")}
					/>

					{form.values.engineType === "quickjs" && (
						<Box>
							<Text size="sm" fw={500} mb={4}>
								Policy Script <span style={{ color: "var(--mantine-color-red-6)" }}>*</span>
							</Text>
							<Text size="xs" c="dimmed" mb="xs">
								Write JavaScript to evaluate access. Use{" "}
								<Code>allow()</Code>, <Code>deny(reason)</Code>,{" "}
								<Code>abstain()</Code>. Press Ctrl+Space for autocomplete.
							</Text>
							<Paper withBorder style={{ overflow: "hidden", borderRadius: 8 }}>
								<PolicyScriptEditor
									value={form.values.script}
									onChange={(val) => form.setFieldValue("script", val)}
									height={200}
								/>
							</Paper>
							{form.errors.script && (
								<Text size="xs" c="red" mt={4}>
									{form.errors.script}
								</Text>
							)}
						</Box>
					)}

					{(form.values.engineType === "deny" || form.values.engineType === "quickjs") && (
						<TextInput
							label="Deny Message"
							placeholder="Custom message when access is denied"
							{...form.getInputProps("denyMessage")}
						/>
					)}

					<Divider
						label={
							<Group
								gap="xs"
								style={{ cursor: "pointer" }}
								onClick={toggleMatcher}
							>
								{matcherExpanded ? (
									<IconChevronDown size={16} />
								) : (
									<IconChevronRight size={16} />
								)}
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

					<Collapse in={matcherExpanded}>
						<Stack gap="md" pt="xs">
							<Group grow align="flex-start">
								<MultiSelect
									label="Roles"
									description="User must have any of these roles"
									placeholder="e.g. admin, practitioner"
									data={[...new Set(["admin", "practitioner", "patient", "nurse", ...form.values.roles])]}
									searchable
									creatable
									getCreateLabel={(query) => `+ Add "${query}"`}
									onCreate={(query) => query}
									{...form.getInputProps("roles")}
								/>
								<MultiSelect
									label="Clients"
									description="OAuth client IDs (supports * wildcard)"
									placeholder="Select or add clients"
									data={clientOptions}
									searchable
									creatable
									getCreateLabel={(query) => `+ Add "${query}"`}
									onCreate={(query) => query}
									{...form.getInputProps("clients")}
								/>
							</Group>

							<Group grow align="flex-start">
								<MultiSelect
									label="User Types"
									description="User's FHIR resource type"
									data={userTypeOptions}
									searchable
									{...form.getInputProps("userTypes")}
								/>
								<MultiSelect
									label="Resource Types"
									description="Target FHIR resource types"
									data={typeOptions}
									searchable
									{...form.getInputProps("resourceTypes")}
								/>
							</Group>

							<MultiSelect
								label="Operations"
								description="FHIR operations to match"
								data={operationOptions}
								searchable
								{...form.getInputProps("operations")}
							/>

							<Group grow align="flex-start">
								<MultiSelect
									label="Operation IDs"
									description="Specific operation IDs (e.g. fhir.read, graphql.query)"
									placeholder="Add operation ID"
									data={form.values.operationIds}
									searchable
									creatable
									getCreateLabel={(query) => `+ Add "${query}"`}
									onCreate={(query) => query}
									{...form.getInputProps("operationIds")}
								/>
								<MultiSelect
									label="Paths"
									description="Request path patterns (glob syntax)"
									placeholder="e.g. /Patient/*, /admin/*"
									data={form.values.paths}
									searchable
									creatable
									getCreateLabel={(query) => `+ Add "${query}"`}
									onCreate={(query) => query}
									{...form.getInputProps("paths")}
								/>
							</Group>

							<MultiSelect
								label="Source IPs"
								description="Client IP addresses in CIDR notation"
								placeholder="e.g. 192.168.1.0/24, 10.0.0.0/8"
								data={form.values.sourceIps}
								searchable
								creatable
								getCreateLabel={(query) => `+ Add "${query}"`}
								onCreate={(query) => query}
								{...form.getInputProps("sourceIps")}
							/>
						</Stack>
					</Collapse>

					<Group justify="flex-end" mt="md">
						<Button variant="light" onClick={onClose}>
							Cancel
						</Button>
						<Button type="submit" loading={create.isPending || update.isPending}>
							{isEditing ? "Update" : "Create"}
						</Button>
					</Group>
				</Stack>
			</form>
		</Modal>
	);
}

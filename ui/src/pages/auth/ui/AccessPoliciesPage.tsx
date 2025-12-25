import { useState, useMemo } from "react";
import {
	Stack,
	Title,
	Text,
	Paper,
	Group,
	Button,
	TextInput,
	Table,
	Badge,
	ActionIcon,
	Menu,
	Modal,
	MultiSelect,
	Textarea,
	Divider,
	Select,
	Box,
	ScrollArea,
} from "@mantine/core";
import { useDisclosure, useDebouncedValue } from "@mantine/hooks";
import { useForm } from "@mantine/form";
import {
	IconPlus,
	IconSearch,
	IconDotsVertical,
	IconEdit,
	IconTrash,
	IconShieldCheck,
	IconCirclePlus,
} from "@tabler/icons-react";
import { useAccessPolicies, useCreateAccessPolicy, useUpdateAccessPolicy, useDeleteAccessPolicy, type AccessPolicyResource, type AccessPolicyRule } from "../lib/useAccessPolicies";
import { useClients } from "../lib/useClients";
import { useResourceTypes } from "@/shared/api/hooks";

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
						Define fine-grained access control rules
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
							<Table.Th>Roles</Table.Th>
							<Table.Th>Rules</Table.Th>
							<Table.Th style={{ width: 50 }} />
						</Table.Tr>
					</Table.Thead>
					<Table.Tbody>
						{isLoading ? (
							<Table.Tr>
								<Table.Td colSpan={4}>Loading...</Table.Td>
							</Table.Tr>
						) : policies.length === 0 ? (
							<Table.Tr>
								<Table.Td colSpan={4} style={{ textAlign: "center" }}>
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
										<Group gap={4}>
											{policy.roles?.map((role) => (
												<Badge key={role} size="sm" variant="outline">
													{role}
												</Badge>
											))}
											{!policy.roles?.length && <Text size="xs" c="dimmed">Any</Text>}
										</Group>
									</Table.Td>
									<Table.Td>
										<Text size="sm">{policy.rules?.length || 0} rules</Text>
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
													onClick={() => handleDelete(policy.id!)}
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

			<PolicyModal
				opened={opened}
				onClose={handleClose}
				policy={editingPolicy}
			/>
		</Stack>
	);
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
	
	const isEditing = !!policy;

	const form = useForm({
		initialValues: {
			name: "",
			description: "",
			roles: [] as string[],
			clients: [] as string[],
			rules: [] as AccessPolicyRule[],
		},
		validate: {
			name: (value) => (value.length < 3 ? "Name must be at least 3 characters" : null),
		},
	});

	useMemo(() => {
		if (policy) {
			form.setValues({
				name: policy.name,
				description: policy.description || "",
				roles: policy.roles || [],
				clients: policy.clients || [],
				rules: policy.rules || [],
			});
		} else {
			form.reset();
			// Add one default rule
			form.insertListItem("rules", {
				resourceTypes: ["*"],
				operations: ["read"],
				allow: true,
			});
		}
	}, [policy]);

	const clientOptions = useMemo(() => {
		return clientsData?.entry?.map(e => ({
			label: e.resource.name,
			value: e.resource.clientId
		})) || [];
	}, [clientsData]);

	const typeOptions = useMemo(() => {
		const types = resourceTypes || [];
		return ["*", ...types];
	}, [resourceTypes]);

	const handleSubmit = async (values: typeof form.values) => {
		const payload: any = {
			resourceType: "AccessPolicy",
			...values,
		};

		try {
			if (isEditing && policy?.id) {
				await update.mutateAsync({ ...payload, id: policy.id });
			} else {
				await create.mutateAsync(payload);
			}
			onClose();
		} catch (e) {
			// Handled by hook
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit Access Policy" : "Create Access Policy"}
			size="xl"
		>
			<form onSubmit={form.onSubmit(handleSubmit)}>
				<Stack gap="md">
					<TextInput
						label="Policy Name"
						required
						{...form.getInputProps("name")}
					/>

					<Textarea
						label="Description"
						{...form.getInputProps("description")}
					/>

					<Group grow>
						<MultiSelect
							label="Target Roles"
							placeholder="e.g. admin, practitioner"
							data={["admin", "practitioner", "patient"]}
							searchable
							creatable
							getCreateLabel={(query) => `+ Add ${query}`}
							onCreate={(query) => query}
							{...form.getInputProps("roles")}
						/>
						<MultiSelect
							label="Target Clients"
							placeholder="Select clients"
							data={clientOptions}
							searchable
							{...form.getInputProps("clients")}
						/>
					</Group>

					<Divider label="Rules" labelPosition="center" />

					<Box>
						<Stack gap="sm">
							{form.values.rules.map((_, index) => (
								<Paper key={index} withBorder p="sm" style={{ backgroundColor: "var(--app-surface-2)" }}>
									<Stack gap="xs">
										<Group justify="space-between">
											<Text size="xs" fw={700}>Rule #{index + 1}</Text>
											<ActionIcon 
												variant="subtle" 
												color="red" 
												size="sm"
												onClick={() => form.removeListItem("rules", index)}
												disabled={form.values.rules.length === 1}
											>
												<IconTrash size={14} />
											</ActionIcon>
										</Group>
										
										<Group grow align="flex-start">
											<MultiSelect
												label="Resource Types"
												data={typeOptions}
												searchable
												{...form.getInputProps(`rules.${index}.resourceTypes`)}
											/>
											<MultiSelect
												label="Operations"
												data={["*", "read", "write", "create", "update", "delete", "search", "history", "$operation"]}
												searchable
												{...form.getInputProps(`rules.${index}.operations`)}
											/>
										</Group>
										
										<Group grow align="flex-end">
											<Select
												label="Effect"
												data={[
													{ label: "Allow", value: "true" },
													{ label: "Deny", value: "false" },
												]}
												value={String(form.values.rules[index].allow)}
												onChange={(val) => form.setFieldValue(`rules.${index}.allow`, val === "true")}
											/>
											<TextInput
												label="Condition (FHIRPath)"
												placeholder="e.g. %user.id = %resource.patient.id"
												{...form.getInputProps(`rules.${index}.condition`)}
											/>
										</Group>
									</Stack>
								</Paper>
							))}
						</Stack>
						
						<Button 
							variant="subtle" 
							leftSection={<IconCirclePlus size={16} />} 
							mt="sm"
							onClick={() => form.insertListItem("rules", {
								resourceTypes: ["*"],
								operations: ["read"],
								allow: true,
							})}
						>
							Add Rule
						</Button>
					</Box>

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

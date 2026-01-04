import { useState, useMemo } from "react";
import { Modal, TextInput, Select, Group, Stack } from "@mantine/core";
import { Button } from "@/shared/ui/Button/Button";
import { useUpdateUser, useSearchResources } from "../lib/useUsers";
import type { UserResource } from "@/shared/api/types";

interface EditUserModalProps {
	user: UserResource;
	opened: boolean;
	onClose: () => void;
}

export function EditUserModal({ user, opened, onClose }: EditUserModalProps) {
	const updateUser = useUpdateUser();
	const [fhirUserSearch, setFhirUserSearch] = useState("");
	const [selectedFhirUser, setSelectedFhirUser] = useState(user.fhirUser?.reference || "");

	// Search for Practitioner and Patient resources
	const { data: practitioners } = useSearchResources("Practitioner", fhirUserSearch);
	const { data: patients } = useSearchResources("Patient", fhirUserSearch);

	// Combine search results
	const resourceOptions = useMemo(() => {
		const options = [];

		// Add practitioners
		if (practitioners?.entry) {
			for (const entry of practitioners.entry) {
				if (entry.resource) {
					const name = entry.resource.name?.[0]?.text || entry.resource.id || "Unknown";
					options.push({
						value: `Practitioner/${entry.resource.id}`,
						label: `Practitioner: ${name}`,
					});
				}
			}
		}

		// Add patients
		if (patients?.entry) {
			for (const entry of patients.entry) {
				if (entry.resource) {
					const name = entry.resource.name?.[0]?.text || entry.resource.id || "Unknown";
					options.push({
						value: `Patient/${entry.resource.id}`,
						label: `Patient: ${name}`,
					});
				}
			}
		}

		return options;
	}, [practitioners, patients]);

	const handleSubmit = () => {
		const updatedUser: UserResource = {
			...user,
			fhirUser: selectedFhirUser ? { reference: selectedFhirUser } : undefined,
		};

		updateUser.mutate(updatedUser, {
			onSuccess: () => {
				onClose();
			},
		});
	};

	return (
		<Modal opened={opened} onClose={onClose} title="Edit User" size="md">
			<Stack gap="md">
				<TextInput label="Username" value={user.username} disabled />

				<TextInput label="Email" value={user.email || ""} disabled />

				<Select
					label="Link to FHIR Resource"
					placeholder="Search Practitioner or Patient..."
					description="Start typing to search for a Practitioner or Patient resource"
					data={resourceOptions}
					searchable
					searchValue={fhirUserSearch}
					onSearchChange={setFhirUserSearch}
					value={selectedFhirUser}
					onChange={(value) => setSelectedFhirUser(value || "")}
					clearable
				/>

				<Group justify="flex-end" mt="xl">
					<Button variant="light" onClick={onClose}>
						Cancel
					</Button>
					<Button onClick={handleSubmit} loading={updateUser.isPending}>
						Save Changes
					</Button>
				</Group>
			</Stack>
		</Modal>
	);
}

import { useState, useMemo } from "react";
import { Modal, TextInput, Select } from "@/shared/ui";
import { Button } from "@/shared/ui/Button/Button";
import { useUpdateUser, useSearchResources } from "../lib/useUsers";
import { getBundleResources, isRecord } from "@/shared/api/guards";
import type { FhirResource, UserResource } from "@/shared/api/types";
import classes from "./EditUserModal.module.css";

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
		const results = [
			...getBundleResources(practitioners, isSearchResource),
			...getBundleResources(patients, isSearchResource),
		];

		return results.map((resource) => ({
			value: `${resource.resourceType}/${resource.id}`,
			label: `${resource.resourceType}: ${getResourceDisplayName(resource)}`,
		}));
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
			<div className={classes.content}>
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

				<div className={classes.actions}>
					<Button variant="light" onClick={onClose}>
						Cancel
					</Button>
					<Button onClick={handleSubmit} loading={updateUser.isPending}>
						Save Changes
					</Button>
				</div>
			</div>
		</Modal>
	);
}

interface SearchResource extends FhirResource {
	resourceType: "Practitioner" | "Patient";
	id: string;
	name?: Array<{ text?: string }>;
}

function isSearchResource(value: unknown): value is SearchResource {
	if (!isRecord(value)) return false;
	if (value.resourceType !== "Practitioner" && value.resourceType !== "Patient") return false;
	if (typeof value.id !== "string" || value.id.length === 0) return false;
	if (value.name !== undefined && !Array.isArray(value.name)) return false;
	return true;
}

function getResourceDisplayName(resource: SearchResource): string {
	const firstName = resource.name?.[0];
	if (isRecord(firstName) && typeof firstName.text === "string" && firstName.text.length > 0) {
		return firstName.text;
	}
	return resource.id;
}

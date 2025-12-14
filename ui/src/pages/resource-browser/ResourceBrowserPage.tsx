import { useState, useMemo } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
	Stack,
	Title,
	Text,
	Paper,
	NavLink,
	ScrollArea,
	Group,
	Badge,
	Loader,
	Center,
	TextInput,
	Card,
} from "@mantine/core";
import { IconFolder, IconSearch, IconFile } from "@tabler/icons-react";
import { useCapabilities, useResourceSearch, useResource } from "@/shared/api/hooks";
import { JsonViewer } from "@/shared/ui-react/JsonViewer";
import type { FhirResource } from "@/shared/api/types";

interface CapabilityStatement {
	resourceType: "CapabilityStatement";
	rest?: Array<{
		resource?: Array<{
			type: string;
		}>;
	}>;
}

export function ResourceBrowserPage() {
	const { type: routeType } = useParams<{ type?: string }>();
	const navigate = useNavigate();

	const [selectedType, setSelectedType] = useState<string | null>(routeType ?? null);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [typeFilter, setTypeFilter] = useState("");

	const { data: capabilities, isLoading: capabilitiesLoading } = useCapabilities();
	const { data: searchBundle, isLoading: searchLoading } = useResourceSearch(
		selectedType ?? "",
		{ _count: 50 },
		{ enabled: !!selectedType },
	);
	const { data: selectedResource, isLoading: resourceLoading } = useResource(
		selectedType ?? "",
		selectedId ?? "",
		{ enabled: !!selectedType && !!selectedId },
	);

	// Extract resource types from capability statement
	const resourceTypes = useMemo(() => {
		const cap = capabilities as CapabilityStatement | undefined;
		const types = cap?.rest?.[0]?.resource?.map((r) => r.type) ?? [];
		return types.sort();
	}, [capabilities]);

	// Filter resource types by search
	const filteredTypes = useMemo(() => {
		if (!typeFilter) return resourceTypes;
		const lower = typeFilter.toLowerCase();
		return resourceTypes.filter((t) => t.toLowerCase().includes(lower));
	}, [resourceTypes, typeFilter]);

	// Extract resources from search bundle
	const resources = useMemo(() => {
		return (searchBundle?.entry?.map((e) => e.resource).filter(Boolean) ?? []) as FhirResource[];
	}, [searchBundle]);

	const handleTypeSelect = (type: string) => {
		setSelectedType(type);
		setSelectedId(null);
		navigate(`/resources/${type}`);
	};

	const handleResourceSelect = (id: string) => {
		setSelectedId(id);
	};

	return (
		<Stack gap="md" h="100%">
			<div>
				<Title order={2}>Resource Browser</Title>
				<Text c="dimmed" size="sm">
					Browse and view FHIR resources
				</Text>
			</div>

			<div style={{ display: "flex", gap: 16, flex: 1, minHeight: 0 }}>
				{/* Resource Types Panel */}
				<Paper withBorder p="sm" style={{ width: 220, display: "flex", flexDirection: "column" }}>
					<Text fw={500} size="sm" mb="xs">
						Resource Types
					</Text>
					<TextInput
						placeholder="Filter types..."
						size="xs"
						mb="sm"
						leftSection={<IconSearch size={14} />}
						value={typeFilter}
						onChange={(e) => setTypeFilter(e.currentTarget.value)}
					/>
					{capabilitiesLoading ? (
						<Center py="md">
							<Loader size="sm" />
						</Center>
					) : (
						<ScrollArea style={{ flex: 1 }}>
							{filteredTypes.map((type) => (
								<NavLink
									key={type}
									label={type}
									leftSection={<IconFolder size={16} />}
									active={selectedType === type}
									onClick={() => handleTypeSelect(type)}
									style={{ borderRadius: 4 }}
								/>
							))}
						</ScrollArea>
					)}
				</Paper>

				{/* Resources List Panel */}
				<Paper withBorder p="sm" style={{ width: 300, display: "flex", flexDirection: "column" }}>
					<Group justify="space-between" mb="sm">
						<Text fw={500} size="sm">
							{selectedType || "Resources"}
						</Text>
						{searchBundle && (
							<Badge size="sm" variant="light">
								{searchBundle.total ?? resources.length}
							</Badge>
						)}
					</Group>

					{searchLoading ? (
						<Center py="md">
							<Loader size="sm" />
						</Center>
					) : !selectedType ? (
						<Text c="dimmed" size="sm" ta="center" py="xl">
							Select a resource type
						</Text>
					) : resources.length === 0 ? (
						<Text c="dimmed" size="sm" ta="center" py="xl">
							No resources found
						</Text>
					) : (
						<ScrollArea style={{ flex: 1 }}>
							<Stack gap="xs">
								{resources.map((resource) => (
									<Card
										key={resource.id}
										withBorder
										padding="xs"
										radius="sm"
										style={{
											cursor: "pointer",
											backgroundColor:
												selectedId === resource.id
													? "var(--mantine-color-primary-light)"
													: undefined,
										}}
										onClick={() => handleResourceSelect(resource.id!)}
									>
										<Group gap="xs">
											<IconFile size={14} />
											<Text size="sm" fw={500}>
												{resource.id}
											</Text>
										</Group>
										{resource.meta?.lastUpdated && (
											<Text size="xs" c="dimmed">
												{new Date(resource.meta.lastUpdated).toLocaleString()}
											</Text>
										)}
									</Card>
								))}
							</Stack>
						</ScrollArea>
					)}
				</Paper>

				{/* Resource Details Panel */}
				<Paper withBorder p="sm" style={{ flex: 1, display: "flex", flexDirection: "column" }}>
					<Text fw={500} size="sm" mb="sm">
						Details
					</Text>

					{resourceLoading ? (
						<Center py="md">
							<Loader size="sm" />
						</Center>
					) : !selectedResource ? (
						<Text c="dimmed" size="sm" ta="center" py="xl">
							Select a resource to view details
						</Text>
					) : (
						<ScrollArea style={{ flex: 1 }}>
							<JsonViewer data={selectedResource} maxHeight="100%" />
						</ScrollArea>
					)}
				</Paper>
			</div>
		</Stack>
	);
}

import {
	Alert,
	Anchor,
	Badge,
	Group,
	Paper,
	ScrollArea,
	Skeleton,
	Stack,
	Table,
	Tabs,
	Text,
} from "@mantine/core";
import { IconAlertCircle, IconCheck, IconX } from "@tabler/icons-react";
import { useNavigate } from "react-router-dom";
import { JsonViewer } from "@/shared/ui-react/JsonViewer";
import type { FhirBundle, FhirResource } from "@/shared/api";
import type { RequestResponse } from "../hooks/useSendConsoleRequest";

interface ResponseViewerProps {
	response?: RequestResponse;
	isLoading?: boolean;
}

interface FhirOperationOutcome {
	resourceType: "OperationOutcome";
	issue?: Array<{
		severity?: "fatal" | "error" | "warning" | "information";
		code?: string;
		diagnostics?: string;
		location?: string[];
	}>;
}

function isFhirOperationOutcome(body: unknown): body is FhirOperationOutcome {
	return (
		typeof body === "object" &&
		body !== null &&
		"resourceType" in body &&
		body.resourceType === "OperationOutcome"
	);
}

function isFhirBundle(body: unknown): body is FhirBundle {
	return (
		typeof body === "object" &&
		body !== null &&
		"resourceType" in body &&
		body.resourceType === "Bundle"
	);
}

export function ResponseViewer({ response, isLoading }: ResponseViewerProps) {
	const navigate = useNavigate();

	if (isLoading) {
		return (
			<Stack gap="md">
				<Skeleton height={60} />
				<Skeleton height={300} />
			</Stack>
		);
	}

	if (!response) {
		return (
			<Stack gap="sm" align="center" py="xl">
				<Text size="sm" c="dimmed">
					No response yet. Send a request to see results.
				</Text>
			</Stack>
		);
	}

	const isSuccess = response.status >= 200 && response.status < 300;
	const isError = response.status >= 400;
	const isOperationOutcome = isFhirOperationOutcome(response.body);

	const bundle = isFhirBundle(response.body) ? response.body : null;
	const bundleEntries = bundle?.entry ?? [];
	const resourceEntries = bundleEntries.filter(
		(entry): entry is { resource: FhirResource; fullUrl?: string } =>
			Boolean(entry.resource && entry.resource.resourceType),
	);
	const hasResultEntries = resourceEntries.length > 0;
	const defaultTab = hasResultEntries ? "results" : "body";

	const handleOpenResource = (resourceType: string, resourceId: string) => {
		navigate(`/resources/${resourceType}/${resourceId}`);
	};

	return (
		<Stack gap="md">
			{/* Status header */}
			<Paper p="sm" style={{ backgroundColor: "var(--app-surface-2)" }}>
				<Group justify="space-between">
					<Group gap="sm">
						<Badge
							color={isSuccess ? "primary" : isError ? "fire" : "warm"}
							leftSection={
								isSuccess ? (
									<IconCheck size={14} />
								) : isError ? (
									<IconX size={14} />
								) : null
							}
						>
							{response.status} {response.statusText}
						</Badge>
						<Text size="sm" c="dimmed">
							{response.durationMs}ms
						</Text>
					</Group>

					<Text size="xs" c="dimmed">
						{new Date(response.requestedAt).toLocaleString()}
					</Text>
				</Group>
			</Paper>

			{/* OperationOutcome extraction */}
			{isError && isOperationOutcome && (
				<Alert color="fire" icon={<IconAlertCircle size={16} />} title="FHIR Error">
					{response.body.issue?.[0]?.diagnostics || "An error occurred"}
					{response.body.issue?.[0]?.severity && (
						<Text size="xs" mt="xs">
							Severity: {response.body.issue[0].severity}
						</Text>
					)}
				</Alert>
			)}

			{/* Response tabs */}
			<Tabs defaultValue={defaultTab} variant="outline">
				<Tabs.List>
					{hasResultEntries && <Tabs.Tab value="results">Results</Tabs.Tab>}
					<Tabs.Tab value="body">Response Body</Tabs.Tab>
					<Tabs.Tab value="headers">Headers</Tabs.Tab>
				</Tabs.List>

				{hasResultEntries && (
					<Tabs.Panel value="results" pt="md">
						<ScrollArea.Autosize mah={320} type="auto">
							<Table striped highlightOnHover>
								<Table.Thead>
									<Table.Tr>
										<Table.Th>Type</Table.Th>
										<Table.Th>ID</Table.Th>
										<Table.Th>Full URL</Table.Th>
									</Table.Tr>
								</Table.Thead>
								<Table.Tbody>
									{resourceEntries.map((entry, index) => {
										const resource = entry.resource;
										const key = resource.id ?? entry.fullUrl ?? `entry-${index}`;
										return (
											<Table.Tr key={key}>
												<Table.Td>
													<Text size="sm" fw={500}>
														{resource.resourceType}
													</Text>
												</Table.Td>
												<Table.Td>
													{resource.id ? (
														<Anchor
															component="button"
															type="button"
															onClick={() =>
																handleOpenResource(resource.resourceType, resource.id)
															}
														>
															{resource.id}
														</Anchor>
													) : (
														<Text size="sm" c="dimmed">
															-
														</Text>
													)}
												</Table.Td>
												<Table.Td>
													<Text size="sm" c="dimmed" lineClamp={1}>
														{entry.fullUrl ?? "-"}
													</Text>
												</Table.Td>
											</Table.Tr>
										);
									})}
								</Table.Tbody>
							</Table>
						</ScrollArea.Autosize>
					</Tabs.Panel>
				)}

				<Tabs.Panel value="body" pt="md">
					{response.body ? (
						<JsonViewer data={response.body} maxHeight={500} />
					) : (
						<Text size="sm" c="dimmed">
							No response body
						</Text>
					)}
				</Tabs.Panel>

				<Tabs.Panel value="headers" pt="md">
					{response.headers ? (
						<Table striped highlightOnHover>
							<Table.Thead>
								<Table.Tr>
									<Table.Th>Header</Table.Th>
									<Table.Th>Value</Table.Th>
								</Table.Tr>
							</Table.Thead>
							<Table.Tbody>
								{Object.entries(response.headers).map(([key, value]) => (
									<Table.Tr key={key}>
										<Table.Td>
											<Text size="sm" fw={500}>
												{key}
											</Text>
										</Table.Td>
										<Table.Td>
											<Text size="sm" c="dimmed">
												{value}
											</Text>
										</Table.Td>
									</Table.Tr>
								))}
							</Table.Tbody>
						</Table>
					) : (
						<Text size="sm" c="dimmed">
							No headers
						</Text>
					)}
				</Tabs.Panel>
			</Tabs>
		</Stack>
	);
}

import { Alert, Badge, Group, Paper, Stack, Text, Skeleton, Tabs, Table } from "@mantine/core";
import { IconAlertCircle, IconCheck, IconX } from "@tabler/icons-react";
import { JsonViewer } from "@/shared/ui-react/JsonViewer";
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

export function ResponseViewer({ response, isLoading }: ResponseViewerProps) {
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

	return (
		<Stack gap="md">
			{/* Status header */}
			<Paper withBorder p="sm">
				<Group justify="space-between">
					<Group gap="sm">
						<Badge
							color={isSuccess ? "green" : isError ? "red" : "yellow"}
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
				<Alert color="red" icon={<IconAlertCircle size={16} />} title="FHIR Error">
					{response.body.issue?.[0]?.diagnostics || "An error occurred"}
					{response.body.issue?.[0]?.severity && (
						<Text size="xs" mt="xs">
							Severity: {response.body.issue[0].severity}
						</Text>
					)}
				</Alert>
			)}

			{/* Response tabs */}
			<Tabs defaultValue="body" variant="outline">
				<Tabs.List>
					<Tabs.Tab value="body">Response Body</Tabs.Tab>
					<Tabs.Tab value="headers">Headers</Tabs.Tab>
				</Tabs.List>

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

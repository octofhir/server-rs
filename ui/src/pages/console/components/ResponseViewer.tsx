import { IconCheck, IconX, OperationOutcomePanel } from "@octofhir/ui-kit";
import { useNavigate } from "react-router-dom";
import { 
	Link, 
	Badge, 
	Flex, 
	Skeleton, 
	Stack, 
	Tabs, 
	Text,
	Box,
	DataPreview,
} from "@/shared/ui";
import {
	getBundleResourceEntries,
	getConsoleResponseStatusTone,
	getResponseDefaultTab,
	isConsoleResponseError,
	isConsoleResponseSuccess,
	isFhirOperationOutcome,
	type RequestResponse,
} from "@/entities/rest-console-response";
import { JsonViewer } from "@/shared/ui-react/JsonViewer";

interface ResponseViewerProps {
	response?: RequestResponse;
	isLoading?: boolean;
}

export function ResponseViewer({ response, isLoading }: ResponseViewerProps) {
	const navigate = useNavigate();

	if (isLoading) {
		return (
			<Stack gap="4" style={{ padding: "20px" }}>
				<Skeleton style={{ height: "40px", borderRadius: "8px" }} />
				<Skeleton style={{ height: "300px", borderRadius: "8px" }} />
			</Stack>
		);
	}

	if (!response) {
		return (
			<Flex direction="column" alignItems="center" style={{ padding: "40px 0", opacity: 0.5 }}>
				<Text variant="body-2">No response data available.</Text>
			</Flex>
		);
	}

	const isSuccess = isConsoleResponseSuccess(response.status);
	const isError = isConsoleResponseError(response.status);
	const operationOutcome = isFhirOperationOutcome(response.body) ? response.body : null;
	const resourceEntries = getBundleResourceEntries(response.body);
	const hasResultEntries = resourceEntries.length > 0;
	const defaultTab = getResponseDefaultTab(response);

	const handleOpenResource = (resourceType: string, resourceId: string) => {
		navigate(`/resources/${resourceType}/${resourceId}`);
	};

	const statusTheme = getConsoleResponseStatusTone(response.status);

	return (
		<Box>
			{/* Status header */}
			<Box style={{ padding: "12px 20px", borderBottom: "1px solid var(--g-color-line-generic-subtle)", backgroundColor: "var(--g-color-base-generic-subtle)" }}>
				<Flex justifyContent="space-between" alignItems="center">
					<Flex gap="3" alignItems="center">
						<Badge
							theme={statusTheme as any}
							size="l"
						>
							<Flex gap="1" alignItems="center">
								{isSuccess ? <IconCheck size={14} /> : isError ? <IconX size={14} /> : null}
								{response.status} {response.statusText}
							</Flex>
						</Badge>
						<Text color="secondary" variant="caption-1">
							{response.durationMs}ms
						</Text>
					</Flex>

					<Text color="secondary" variant="caption-1">
						{new Date(response.requestedAt).toLocaleString()}
					</Text>
				</Flex>
			</Box>

			{/* OperationOutcome extraction */}
			{isError && operationOutcome && (
				<Box style={{ padding: "16px" }}>
					<OperationOutcomePanel outcome={operationOutcome} title="FHIR error" maxIssues={4} />
				</Box>
			)}

			{/* Response tabs */}
			<Box style={{ padding: "0 20px 20px 20px" }}>
				<Tabs defaultValue={defaultTab} size="l">
					<Tabs.List>
						{hasResultEntries && <Tabs.Tab value="results">Results</Tabs.Tab>}
						<Tabs.Tab value="body">Response Body</Tabs.Tab>
						<Tabs.Tab value="headers">Headers</Tabs.Tab>
					</Tabs.List>

					{hasResultEntries && (
						<Tabs.Panel value="results" style={{ paddingTop: "16px" }}>
							<DataPreview
								columns={[
									{ id: "type", label: "Type", width: 160 },
									{ id: "id", label: "ID", width: 220 },
									{ id: "fullUrl", label: "Full URL" },
								]}
								rows={resourceEntries.map((entry) => ({
									type: (
										<Text variant="body-2" style={{ fontWeight: 500 }}>
											{entry.resource.resourceType}
										</Text>
									),
									id: entry.resource.id ? (
										<Link
											onClick={() =>
												handleOpenResource(entry.resource.resourceType, entry.resource.id)
											}
											style={{ cursor: "pointer" }}
										>
											{entry.resource.id}
										</Link>
									) : (
										<Text color="secondary">-</Text>
									),
									fullUrl: (
										<Text color="secondary" variant="caption-1">
											{entry.fullUrl ?? "-"}
										</Text>
									),
								}))}
								getRowKey={(row, index) =>
									resourceEntries[index]?.resource.id ??
									resourceEntries[index]?.fullUrl ??
									`entry-${index}`
								}
							/>
						</Tabs.Panel>
					)}

					<Tabs.Panel value="body" style={{ paddingTop: "16px" }}>
						{response.body ? (
							<Box style={{ borderRadius: "8px", overflow: "hidden", border: "1px solid var(--g-color-line-generic-subtle)" }}>
								<JsonViewer data={response.body} maxHeight={600} />
							</Box>
						) : (
							<Text color="secondary">No response body</Text>
						)}
					</Tabs.Panel>

					<Tabs.Panel value="headers" style={{ paddingTop: "16px" }}>
						{response.headers ? (
							<DataPreview
								columns={[
									{ id: "header", label: "Header", width: 260 },
									{ id: "value", label: "Value" },
								]}
								rows={Object.entries(response.headers).map(([key, value]) => ({
									header: (
										<Text variant="body-2" style={{ fontWeight: 500 }}>
											{key}
										</Text>
									),
									value: (
										<Text color="secondary" variant="body-2">
											{value}
										</Text>
									),
								}))}
								getRowKey={(_row, index) => Object.keys(response.headers ?? {})[index] ?? `${index}`}
							/>
						) : (
							<Text color="secondary">No headers</Text>
						)}
					</Tabs.Panel>
				</Tabs>
			</Box>
		</Box>
	);
}

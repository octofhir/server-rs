import { IconCheck, IconX, OperationOutcomePanel } from "@octofhir/ui-kit";
import { useNavigate } from "react-router-dom";
import { 
	Link, 
	Badge, 
	Skeleton, 
	Tabs, 
	Text,
	DataPreview,
} from "@octofhir/ui-kit";
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
import styles from "./ResponseViewer.module.css";

interface ResponseViewerProps {
	response?: RequestResponse;
	isLoading?: boolean;
}

export function ResponseViewer({ response, isLoading }: ResponseViewerProps) {
	const navigate = useNavigate();

	if (isLoading) {
		return (
			<div className={styles.loading}>
				<Skeleton className={styles.skeletonHeader} />
				<Skeleton className={styles.skeletonBody} />
			</div>
		);
	}

	if (!response) {
		return (
			<div className={styles.empty}>
				<Text variant="body-2">No response data available.</Text>
			</div>
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
		<div>
			{/* Status header */}
			<div className={styles.header}>
					<div className={styles.status}>
						<Badge
							theme={statusTheme}
							size="lg"
						>
							<span className={styles.statusBadge}>
								{isSuccess ? <IconCheck size={14} /> : isError ? <IconX size={14} /> : null}
								{response.status} {response.statusText}
							</span>
						</Badge>
						<Text color="secondary" variant="caption-1">
							{response.durationMs}ms
						</Text>
					</div>

					<Text color="secondary" variant="caption-1">
						{new Date(response.requestedAt).toLocaleString()}
					</Text>
			</div>

			{/* OperationOutcome extraction */}
			{isError && operationOutcome && (
				<div className={styles.outcome}>
					<OperationOutcomePanel outcome={operationOutcome} title="FHIR error" maxIssues={4} />
				</div>
			)}

			{/* Response tabs */}
			<div className={styles.tabs}>
				<Tabs defaultValue={defaultTab} size="lg">
					<Tabs.List>
						{hasResultEntries && <Tabs.Tab value="results">Results</Tabs.Tab>}
						<Tabs.Tab value="body">Response Body</Tabs.Tab>
						<Tabs.Tab value="headers">Headers</Tabs.Tab>
					</Tabs.List>

					{hasResultEntries && (
						<Tabs.Panel value="results" className={styles.tabPanel}>
							<DataPreview
								columns={[
									{ id: "type", label: "Type", width: 160 },
									{ id: "id", label: "ID", width: 220 },
									{ id: "fullUrl", label: "Full URL" },
								]}
								rows={resourceEntries.map((entry) => ({
									type: (
										<Text variant="body-2" className={styles.tableLabel}>
											{entry.resource.resourceType}
										</Text>
									),
									id: entry.resource.id ? (
										<Link
											onClick={() =>
												handleOpenResource(entry.resource.resourceType, entry.resource.id as string)
											}
											className={styles.resourceLink}
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
								getRowKey={(_row, index) =>
									resourceEntries[index]?.resource.id ??
									resourceEntries[index]?.fullUrl ??
									`entry-${index}`
								}
							/>
						</Tabs.Panel>
					)}

					<Tabs.Panel value="body" className={styles.tabPanel}>
						{response.body ? (
							<div className={styles.jsonFrame}>
								<JsonViewer data={response.body} maxHeight={600} />
							</div>
						) : (
							<Text color="secondary">No response body</Text>
						)}
					</Tabs.Panel>

					<Tabs.Panel value="headers" className={styles.tabPanel}>
						{response.headers ? (
							<DataPreview
								columns={[
									{ id: "header", label: "Header", width: 260 },
									{ id: "value", label: "Value" },
								]}
								rows={Object.entries(response.headers).map(([key, value]) => ({
									header: (
										<Text variant="body-2" className={styles.tableLabel}>
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
			</div>
		</div>
	);
}

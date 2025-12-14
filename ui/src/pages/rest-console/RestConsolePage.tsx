import { useState, useCallback } from "react";
import {
	Stack,
	Title,
	Text,
	Group,
	Select,
	TextInput,
	Button,
	Paper,
	Badge,
	Textarea,
	Loader,
	Alert,
	Code,
	ScrollArea,
} from "@mantine/core";
import { IconSend, IconAlertCircle } from "@tabler/icons-react";
import { fhirClient } from "@/shared/api";

type HttpMethod = "GET" | "POST" | "PUT" | "DELETE" | "PATCH";

const METHOD_OPTIONS = [
	{ value: "GET", label: "GET" },
	{ value: "POST", label: "POST" },
	{ value: "PUT", label: "PUT" },
	{ value: "DELETE", label: "DELETE" },
	{ value: "PATCH", label: "PATCH" },
];

interface ResponseState {
	data: unknown;
	status: number;
	statusText: string;
	responseTime: number;
}

export function RestConsolePage() {
	const [method, setMethod] = useState<HttpMethod>("GET");
	const [path, setPath] = useState("/Patient");
	const [requestBody, setRequestBody] = useState("");
	const [response, setResponse] = useState<ResponseState | null>(null);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const handleSubmit = useCallback(async () => {
		setLoading(true);
		setError(null);
		setResponse(null);

		try {
			let body: unknown;
			if (requestBody && ["POST", "PUT", "PATCH"].includes(method)) {
				body = JSON.parse(requestBody);
			}

			const result = await fhirClient.rawRequest(method, path, body);
			setResponse({
				data: result.data,
				status: result.status,
				statusText: result.statusText,
				responseTime: Math.round(result.responseTime),
			});
		} catch (err) {
			if (err instanceof SyntaxError) {
				setError("Invalid JSON in request body");
			} else {
				setError(err instanceof Error ? err.message : "Request failed");
			}
		} finally {
			setLoading(false);
		}
	}, [method, path, requestBody]);

	const handleKeyDown = useCallback(
		(e: React.KeyboardEvent) => {
			if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
				handleSubmit();
			}
		},
		[handleSubmit],
	);

	const needsBody = ["POST", "PUT", "PATCH"].includes(method);

	const getStatusColor = (status: number) => {
		if (status < 300) return "green";
		if (status < 400) return "yellow";
		return "red";
	};

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0, height: "100%" }}>
			<div>
				<Title order={2}>REST Console</Title>
				<Text c="dimmed" size="sm">
					Test and interact with FHIR REST API endpoints
				</Text>
			</div>

			<Paper withBorder p="md">
				<Stack gap="md">
					<Group gap="sm" align="flex-end">
						<Select
							label="Method"
							data={METHOD_OPTIONS}
							value={method}
							onChange={(v) => setMethod((v as HttpMethod) || "GET")}
							w={120}
							allowDeselect={false}
						/>
						<TextInput
							label="Path"
							value={path}
							onChange={(e) => setPath(e.currentTarget.value)}
							onKeyDown={handleKeyDown}
							placeholder="/Patient"
							style={{ flex: 1 }}
						/>
						<Button
							leftSection={<IconSend size={16} />}
							onClick={handleSubmit}
							loading={loading}
						>
							Send
						</Button>
					</Group>

					{needsBody && (
						<Textarea
							label="Request Body (JSON)"
							value={requestBody}
							onChange={(e) => setRequestBody(e.currentTarget.value)}
							placeholder='{"resourceType": "Patient", ...}'
							minRows={6}
							autosize
							maxRows={12}
							styles={{ input: { fontFamily: "var(--mantine-font-family-monospace)" } }}
						/>
					)}
				</Stack>
			</Paper>

			<Paper withBorder p="md" style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
				<Group justify="space-between" mb="md">
					<Text fw={500}>Response</Text>
					{response && (
						<Group gap="xs">
							<Badge color={getStatusColor(response.status)} variant="light">
								{response.status} {response.statusText}
							</Badge>
							<Badge color="gray" variant="light">
								{response.responseTime}ms
							</Badge>
						</Group>
					)}
				</Group>

				{loading && (
					<Group justify="center" py="xl">
						<Loader size="sm" />
						<Text size="sm" c="dimmed">
							Sending request...
						</Text>
					</Group>
				)}

				{error && (
					<Alert icon={<IconAlertCircle size={16} />} color="red" variant="light">
						{error}
					</Alert>
				)}

				{response && (
					<ScrollArea style={{ flex: 1 }}>
						<Code block style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
							{JSON.stringify(response.data, null, 2)}
						</Code>
					</ScrollArea>
				)}

				{!loading && !error && !response && (
					<Text c="dimmed" ta="center" py="xl">
						Send a request to see the response
					</Text>
				)}
			</Paper>
		</Stack>
	);
}

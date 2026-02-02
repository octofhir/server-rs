import { useState } from "react";
import {
	Alert,
	Button,
	Group,
	Paper,
	Stack,
	Text,
	Title,
} from "@mantine/core";
import { IconAlertCircle, IconPlayerPlay, IconX } from "@tabler/icons-react";
import { useMutation } from "@tanstack/react-query";
import { FhirPathEditor } from "@/shared/monaco/FhirPathEditor";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { MetadataPanel } from "./components/MetadataPanel";
import { ResultItem } from "./components/ResultItem";
import {
	parseParametersResponse,
	type FhirPathEvaluationResponse,
} from "./types";

export function FhirPathConsolePage() {
	const [expression, setExpression] = useState("Patient.name.given");
	const [inputResource, setInputResource] = useState(
		JSON.stringify(
			{
				resourceType: "Patient",
				name: [
					{
						family: "Smith",
						given: ["John", "Bob"],
					},
				],
			},
			null,
			2,
		),
	);

	const evaluateMutation = useMutation<FhirPathEvaluationResponse, Error>({
		mutationFn: async () => {
			const body: {
				resourceType: string;
				parameter: Array<{
					name: string;
					valueString?: string;
					resource?: unknown;
				}>;
			} = {
				resourceType: "Parameters",
				parameter: [{ name: "expression", valueString: expression }],
			};

			if (inputResource.trim()) {
				try {
					const resource = JSON.parse(inputResource);
					body.parameter.push({ name: "resource", resource });
				} catch (e) {
					throw new Error("Invalid JSON in input resource");
				}
			}

			const response = await fetch("/fhir/$fhirpath", {
				method: "POST",
				headers: {
					"Content-Type": "application/fhir+json",
				},
				body: JSON.stringify(body),
			});

			if (!response.ok) {
				const error = await response.text();
				throw new Error(`HTTP ${response.status}: ${error}`);
			}

			const params = await response.json();
			return parseParametersResponse(params);
		},
	});

	const handleExecute = () => {
		evaluateMutation.mutate();
	};

	const handleClear = () => {
		setExpression("");
		setInputResource("");
		evaluateMutation.reset();
	};

	return (
		<Stack gap="md" h="100%" p="md" className="page-enter">
			<Group justify="space-between">
				<Title order={2}>FHIRPath Console</Title>
			</Group>

			{/* Expression Editor */}
			<Stack gap="xs">
				<Text size="sm" fw={500}>
					Expression
				</Text>
				<Paper withBorder p={0}>
					<FhirPathEditor
						value={expression}
						onChange={(value) => setExpression(value)}
						onSubmit={handleExecute}
						height={120}
						placeholder="Enter FHIRPath expression (e.g., Patient.name.given)"
					/>
				</Paper>
			</Stack>

			{/* Input Resource */}
			<Stack gap="xs" style={{ flex: "0 0 300px" }}>
				<Text size="sm" fw={500}>
					Input Resource (optional)
				</Text>
				<JsonEditor
					value={inputResource}
					onChange={(value) => setInputResource(value)}
					onExecute={handleExecute}
					height={300}
				/>
			</Stack>

			{/* Actions */}
			<Group>
				<Button
					leftSection={<IconPlayerPlay size={16} />}
					onClick={handleExecute}
					loading={evaluateMutation.isPending}
				>
					Execute (Ctrl+Enter)
				</Button>
				<Button
					variant="subtle"
					leftSection={<IconX size={16} />}
					onClick={handleClear}
				>
					Clear
				</Button>
			</Group>

			{/* Results */}
			<Stack gap="sm" style={{ flex: 1, minHeight: 0, overflow: "auto" }}>
				{evaluateMutation.error && (
					<Alert icon={<IconAlertCircle />} color="red">
						<Text fw={500}>Evaluation Error</Text>
						<Text size="sm">{evaluateMutation.error.message}</Text>
					</Alert>
				)}

				{evaluateMutation.data && (
					<>
						<MetadataPanel metadata={evaluateMutation.data.metadata} />

						<Paper withBorder p="md">
							<Group justify="space-between" mb="sm">
								<Text fw={500}>
									Results ({evaluateMutation.data.results.length})
								</Text>
							</Group>

							<Stack gap="sm">
								{evaluateMutation.data.results.length === 0 ? (
									<Text c="dimmed" size="sm">
										No results (empty collection)
									</Text>
								) : (
									evaluateMutation.data.results.map((result) => (
										<ResultItem key={result.index} result={result} />
									))
								)}
							</Stack>
						</Paper>
					</>
				)}
			</Stack>
		</Stack>
	);
}

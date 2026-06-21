import { useState } from "react";
import {
	Alert,
	Button,
	Text,
	Resizable,
} from "@octofhir/ui-kit";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import { CircleAlert as CircleExclamation, Play, X as Xmark } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { FhirPathEditor } from "@/shared/monaco/FhirPathEditor";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { MetadataPanel } from "./components/MetadataPanel";
import { ResultItem } from "./components/ResultItem";
import {
	parseParametersResponse,
	type FhirPathEvaluationResponse,
} from "./types";
import { assertFhirResource } from "@/shared/api/guards";
import classes from "./FhirPathConsolePage.module.css";

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
					const resource = assertFhirResource(JSON.parse(inputResource), "FHIRPath input resource");
					body.parameter.push({ name: "resource", resource });
				} catch {
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
		<ToolWorkspaceLayout
			title="FHIRPath Console"
			description="Evaluate FHIRPath expressions against a sample or pasted resource"
			className="page-enter"
			actions={
				<div className={classes.actions}>
					<Button
						leftSection={<Play size={16} />}
						onClick={handleExecute}
						loading={evaluateMutation.isPending}
					>
						Execute
					</Button>
					<Button
						variant="subtle"
						leftSection={<Xmark size={16} />}
						onClick={handleClear}
					>
						Clear
					</Button>
				</div>
			}
		>
			<div className={classes.workspaceResizable}>
				<Resizable.Group orientation="vertical">
					<Resizable.Pane defaultSize={20} minSize={10}>
						<div className={classes.editorPanel} style={{ height: "100%", overflow: "hidden" }}>
							<Text size="sm" fw={500} className={classes.panelTitle}>
								Expression
							</Text>
							<FhirPathEditor
								value={expression}
								onChange={(value) => setExpression(value)}
								onSubmit={handleExecute}
								height={120}
								placeholder="Enter FHIRPath expression (e.g., Patient.name.given)"
							/>
						</div>
					</Resizable.Pane>

					<Resizable.Handle />

					<Resizable.Pane defaultSize={40} minSize={20}>
						<div className={classes.editorPanel} style={{ height: "100%", overflow: "hidden" }}>
							<Text size="sm" fw={500} className={classes.panelTitle}>
								Input Resource
							</Text>
							<JsonEditor
								value={inputResource}
								onChange={(value) => setInputResource(value)}
								onExecute={handleExecute}
								height={300}
							/>
						</div>
					</Resizable.Pane>

					<Resizable.Handle />

					<Resizable.Pane defaultSize={40} minSize={20}>
						<div className={classes.resultsPanel} style={{ height: "100%", overflow: "auto" }}>
							{evaluateMutation.error && (
								<Alert icon={<CircleExclamation />} color="red">
									<Text fw={500}>Evaluation Error</Text>
									<Text size="sm">{evaluateMutation.error.message}</Text>
								</Alert>
							)}

							{evaluateMutation.data ? (
								<>
									<MetadataPanel metadata={evaluateMutation.data.metadata} />

									<div className={classes.panel}>
										<Text fw={500} className={classes.panelTitle}>
											Results ({evaluateMutation.data.results.length})
										</Text>

										<div className={classes.resultList}>
											{evaluateMutation.data.results.length === 0 ? (
												<Text c="dimmed" size="sm">
													No results (empty collection)
												</Text>
											) : (
												evaluateMutation.data.results.map((result) => (
													<ResultItem key={result.index} result={result} />
												))
											)}
										</div>
									</div>
								</>
							) : (
								<div className={classes.emptyState}>
									<Text size="sm" c="dimmed">
										Run an expression to see evaluation metadata and results.
									</Text>
								</div>
							)}
						</div>
					</Resizable.Pane>
				</Resizable.Group>
			</div>
		</ToolWorkspaceLayout>
	);
}

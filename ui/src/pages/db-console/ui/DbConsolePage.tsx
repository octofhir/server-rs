import { createSignal, For, Show } from "solid-js";

import { serverApi, ApiResponseError } from "@/shared/api/serverApi";
import type { SqlResponse, SqlValue, FhirOperationOutcome } from "@/shared/api/types";
import { SqlEditor } from "@/shared/monaco";
import { Button } from "@/shared/ui";

import styles from "./DbConsolePage.module.css";

export const DbConsolePage = () => {
	const [query, setQuery] = createSignal("SELECT * FROM patient LIMIT 10;");
	const [results, setResults] = createSignal<SqlResponse | null>(null);
	const [error, setError] = createSignal<string | null>(null);
	const [errorDetail, setErrorDetail] = createSignal<FhirOperationOutcome | null>(null);
	const [loading, setLoading] = createSignal(false);

	const handleExecute = async () => {
		setLoading(true);
		setError(null);
		setErrorDetail(null);

		try {
			const result = await serverApi.executeSql(query());
			setResults(result);
		} catch (err) {
			// Extract error details from ApiResponseError
			if (err instanceof ApiResponseError) {
				// Check if response data is a FHIR OperationOutcome
				if (err.responseData?.resourceType === "OperationOutcome") {
					setErrorDetail(err.responseData as FhirOperationOutcome);
					// Extract first issue's diagnostics as primary error message
					const firstIssue = err.responseData.issue?.[0];
					setError(firstIssue?.diagnostics || err.message);
				} else {
					setError(err.message);
				}
			} else if (err instanceof Error) {
				setError(err.message);
			} else {
				setError("Query execution failed");
			}
			setResults(null);
		} finally {
			setLoading(false);
		}
	};

	const formatCellValue = (value: SqlValue): string => {
		if (value === null) return "NULL";
		if (typeof value === "object") return JSON.stringify(value);
		return String(value);
	};

	return (
		<div class={styles.container}>
			<div class={styles.header}>
				<h1 class={styles.title}>DB Console</h1>
				<div class={styles.actions}>
					<Button onClick={handleExecute} loading={loading()}>
						Execute (Ctrl+Enter)
					</Button>
				</div>
			</div>

			<div class={styles.editorContainer}>
				<div class={styles.editorToolbar}>
					<span class={styles.editorLabel}>SQL Editor</span>
				</div>
				<div class={styles.editor}>
					<SqlEditor
						value={query()}
						onChange={setQuery}
						enableLsp
						onExecute={handleExecute}
					/>
				</div>
			</div>

			<div class={styles.resultsContainer}>
				<div class={styles.resultsHeader}>
					<span>Results</span>
					<Show when={results()} keyed>
						{(res) => (
							<span class={styles.resultsMeta}>
								{res.rowCount} rows in {res.executionTimeMs}ms
							</span>
						)}
					</Show>
				</div>
				<div class={styles.resultsContent}>
					<Show when={error()}>
						<div class={styles.errorContainer}>
							<div class={styles.errorState}>
								<span class={styles.errorIcon}>!</span>
								<span>{error()}</span>
							</div>
							<Show when={errorDetail()}>
								<details class={styles.errorDetails}>
									<summary class={styles.errorDetailsSummary}>
										Show full OperationOutcome
									</summary>
									<pre class={styles.errorDetailsPre}>
										{JSON.stringify(errorDetail(), null, 2)}
									</pre>
								</details>
							</Show>
						</div>
					</Show>
					<Show
						when={results()}
						keyed
						fallback={
							<Show when={!error()}>
								<div class={styles.emptyState}>Run a query to see results</div>
							</Show>
						}
					>
						{(res) => (
							<Show
								when={res.rowCount > 0}
								fallback={
									<div class={styles.emptyState}>
										<span class={styles.infoIcon}>ℹ</span>
										<span>Query executed successfully. No rows returned.</span>
									</div>
								}
							>
								<div class={styles.tableWrapper}>
									<table class={styles.resultsTable}>
										<thead>
											<tr>
												<For each={res.columns}>{(col) => <th>{col}</th>}</For>
											</tr>
										</thead>
										<tbody>
											<For each={res.rows}>
												{(row) => (
													<tr>
														<For each={row}>
															{(cell) => (
																<td class={cell === null ? styles.nullCell : ""}>
																	{formatCellValue(cell)}
																</td>
															)}
														</For>
													</tr>
												)}
											</For>
										</tbody>
									</table>
								</div>
							</Show>
						)}
					</Show>
				</div>
			</div>
		</div>
	);
};

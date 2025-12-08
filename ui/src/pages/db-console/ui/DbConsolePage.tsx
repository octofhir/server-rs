import { createSignal, For, Show } from "solid-js";

import { serverApi } from "@/shared/api/serverApi";
import type { SqlResponse, SqlValue } from "@/shared/api/types";
import { SqlEditor } from "@/shared/monaco";
import { Button } from "@/shared/ui";

import styles from "./DbConsolePage.module.css";

export const DbConsolePage = () => {
    const [query, setQuery] = createSignal("SELECT * FROM patient LIMIT 10;");
    const [results, setResults] = createSignal<SqlResponse | null>(null);
    const [error, setError] = createSignal<string | null>(null);
    const [loading, setLoading] = createSignal(false);

    const handleExecute = async () => {
        setLoading(true);
        setError(null);

        try {
            const result = await serverApi.executeSql(query());
            setResults(result);
        } catch (err) {
            setError(err instanceof Error ? err.message : "Query execution failed");
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
                        <div class={styles.errorState}>
                            <span class={styles.errorIcon}>!</span>
                            {error()}
                        </div>
                    </Show>
                    <Show
                        when={results()}
                        keyed
                        fallback={
                            <Show when={!error()}>
                                <div class={styles.emptyState}>
                                    Run a query to see results
                                </div>
                            </Show>
                        }
                    >
                        {(res) => (
                            <div class={styles.tableWrapper}>
                                <table class={styles.resultsTable}>
                                    <thead>
                                        <tr>
                                            <For each={res.columns}>
                                                {(col) => <th>{col}</th>}
                                            </For>
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
                        )}
                    </Show>
                </div>
            </div>
        </div>
    );
};

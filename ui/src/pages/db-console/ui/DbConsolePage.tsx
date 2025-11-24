import { createSignal, onMount, onCleanup } from "solid-js";
import { EditorView, basicSetup } from "codemirror";
import { sql } from "@codemirror/lang-sql";
import { EditorState } from "@codemirror/state";
import styles from "./DbConsolePage.module.css";

export const DbConsolePage = () => {
    let editorRef: HTMLDivElement | undefined;
    let view: EditorView | undefined;
    const [query, setQuery] = createSignal("SELECT * FROM Patient LIMIT 10;");
    const [results, setResults] = createSignal<any[] | null>(null);
    const [loading, setLoading] = createSignal(false);

    onMount(() => {
        if (!editorRef) return;

        const state = EditorState.create({
            doc: query(),
            extensions: [
                basicSetup,
                sql(),
                EditorView.updateListener.of((update) => {
                    if (update.docChanged) {
                        setQuery(update.state.doc.toString());
                    }
                }),
                EditorView.theme({
                    "&": { height: "100%", fontSize: "14px" },
                    ".cm-content": { fontFamily: "var(--font-mono)" },
                    ".cm-gutters": {
                        backgroundColor: "var(--bg-secondary)",
                        color: "var(--text-muted)",
                        borderRight: "1px solid var(--border-subtle)"
                    },
                    "&.cm-focused": { outline: "none" }
                })
            ],
        });

        view = new EditorView({
            state,
            parent: editorRef,
        });
    });

    onCleanup(() => {
        view?.destroy();
    });

    const handleExecute = async () => {
        setLoading(true);
        // Mock execution for now
        setTimeout(() => {
            setResults([
                { id: "1", resourceType: "Patient", name: "John Doe" },
                { id: "2", resourceType: "Patient", name: "Jane Smith" },
            ]);
            setLoading(false);
        }, 500);
    };

    return (
        <div class={styles.container}>
            <div class={styles.header}>
                <h1 class={styles.title}>DB Console</h1>
                <div class={styles.actions}>
                    <button
                        class="btn-primary" // Using simple class if Button component not found or complex
                        onClick={handleExecute}
                        disabled={loading()}
                        style={{
                            "background-color": "var(--primary-color)",
                            "color": "white",
                            "border": "none",
                            "padding": "8px 16px",
                            "border-radius": "var(--radius-md)",
                            "font-weight": "500",
                            "cursor": "pointer",
                            "opacity": loading() ? "0.7" : "1"
                        }}
                    >
                        {loading() ? "Running..." : "Execute Query"}
                    </button>
                </div>
            </div>

            <div class={styles.editorContainer}>
                <div class={styles.editorToolbar}>
                    <span class={styles.editorLabel}>SQL Editor</span>
                </div>
                <div ref={editorRef} class={styles.editor} />
            </div>

            <div class={styles.resultsContainer}>
                <div class={styles.resultsHeader}>Results</div>
                <div class={styles.resultsContent}>
                    {results() ? (
                        <pre style={{ "font-family": "var(--font-mono)", "font-size": "var(--font-size-sm)" }}>
                            {JSON.stringify(results(), null, 2)}
                        </pre>
                    ) : (
                        <div class={styles.emptyState}>
                            Run a query to see results
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
};

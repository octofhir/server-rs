import { onMount, onCleanup, createSignal } from "solid-js";
import styles from "./GraphQLConsolePage.module.css";

/**
 * GraphQL Console page that loads GraphiQL from CDN.
 * This provides a full-featured GraphQL IDE without needing React as a dependency.
 */
export const GraphQLConsolePage = () => {
	let containerRef: HTMLDivElement | undefined;
	const [loading, setLoading] = createSignal(true);
	const [error, setError] = createSignal<string | null>(null);

	onMount(async () => {
		if (!containerRef) return;

		try {
			// Load GraphiQL CSS
			const cssLink = document.createElement("link");
			cssLink.rel = "stylesheet";
			cssLink.href =
				"https://unpkg.com/graphiql@3/graphiql.min.css";
			document.head.appendChild(cssLink);

			// Load React and ReactDOM first (required by GraphiQL)
			await loadScript(
				"https://unpkg.com/react@18/umd/react.production.min.js",
			);
			await loadScript(
				"https://unpkg.com/react-dom@18/umd/react-dom.production.min.js",
			);

			// Load GraphiQL
			await loadScript("https://unpkg.com/graphiql@3/graphiql.min.js");

			// Create the fetcher
			const graphQLEndpoint = `${window.location.origin}/fhir/$graphql`;

			// biome-ignore lint/suspicious/noExplicitAny: GraphiQL global from CDN
			const GraphiQL = (window as any).GraphiQL;
			// biome-ignore lint/suspicious/noExplicitAny: React global from CDN
			const React = (window as any).React;
			// biome-ignore lint/suspicious/noExplicitAny: ReactDOM global from CDN
			const ReactDOM = (window as any).ReactDOM;

			if (!GraphiQL || !React || !ReactDOM) {
				throw new Error("Failed to load GraphiQL dependencies");
			}

			// Custom fetcher that includes credentials for auth
			const fetcher = async (graphQLParams: {
				query: string;
				variables?: Record<string, unknown>;
				operationName?: string;
			}) => {
				const response = await fetch(graphQLEndpoint, {
					method: "POST",
					headers: {
						Accept: "application/json",
						"Content-Type": "application/json",
					},
					credentials: "include",
					body: JSON.stringify(graphQLParams),
				});
				return response.json();
			};

			// Default query
			const defaultQuery = `# Welcome to the OctoFHIR GraphQL Console!
#
# GraphiQL is an in-browser tool for writing, validating, and
# testing GraphQL queries.
#
# Keyboard shortcuts:
#   Prettify query:  Shift-Ctrl-P (or press the prettify button)
#   Run Query:       Ctrl-Enter (or press the play button)
#   Auto Complete:   Ctrl-Space (or just start typing)
#

query HealthCheck {
  _health {
    status
  }
  _version
}

# Example: Get patients
# query {
#   PatientList(_count: 5) {
#     id
#     gender
#     birthDate
#     name { family given }
#   }
# }
`;

			// Render GraphiQL
			const root = ReactDOM.createRoot(containerRef);
			root.render(
				React.createElement(GraphiQL, {
					fetcher,
					defaultQuery,
					headerEditorEnabled: true,
					shouldPersistHeaders: true,
				}),
			);

			setLoading(false);
		} catch (err) {
			console.error("Failed to load GraphiQL:", err);
			setError(err instanceof Error ? err.message : "Failed to load GraphiQL");
			setLoading(false);
		}
	});

	onCleanup(() => {
		// Clean up React root if needed
		if (containerRef) {
			// biome-ignore lint/suspicious/noExplicitAny: ReactDOM global from CDN
			const ReactDOM = (window as any).ReactDOM;
			if (ReactDOM?.createRoot) {
				try {
					// React 18 cleanup
					const root = ReactDOM.createRoot(containerRef);
					root.unmount?.();
				} catch {
					// Ignore cleanup errors
				}
			}
		}
	});

	return (
		<div class={styles.container}>
			{loading() && (
				<div class={styles.loadingState}>
					<div class={styles.spinner} />
					<span>Loading GraphiQL...</span>
				</div>
			)}
			{error() && (
				<div class={styles.errorState}>
					<span class={styles.errorIcon}>!</span>
					<span>{error()}</span>
				</div>
			)}
			<div
				ref={containerRef}
				class={styles.graphiqlContainer}
				style={{ display: loading() || error() ? "none" : "flex" }}
			/>
		</div>
	);
};

/**
 * Helper to load a script dynamically and wait for it to load.
 */
function loadScript(src: string): Promise<void> {
	return new Promise((resolve, reject) => {
		// Check if already loaded
		const existing = document.querySelector(`script[src="${src}"]`);
		if (existing) {
			resolve();
			return;
		}

		const script = document.createElement("script");
		script.src = src;
		script.async = true;
		script.onload = () => resolve();
		script.onerror = () => reject(new Error(`Failed to load script: ${src}`));
		document.head.appendChild(script);
	});
}

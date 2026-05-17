import { useMemo } from "react";
import { GraphiQL } from "graphiql";
import { createGraphiQLFetcher } from "@graphiql/toolkit";
import { useColorScheme } from "@octofhir/ui-kit";
import "graphiql/style.css";
import { useUiSettings } from "@/shared";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";

const DEFAULT_QUERY = `# Welcome to the Abyxon GraphQL Console!
#
# GraphiQL is an in-browser tool for writing, validating, and
# testing GraphQL queries.
#
# Keyboard shortcuts:
#   Prettify query:  Shift-Ctrl-P (or press the prettify button)
#   Run Query:       Ctrl-Enter (or press the play button)
#   Auto Complete:   Ctrl-Space (or just start typing)
#

query {
  PatientList(_count: 5) {
     id
     gender
     birthDate
     name { family given }
   }
 }
`;
const graphQLEndpoint = `${window.location.origin}/$graphql`;

export function GraphQLConsolePage() {
	const { colorScheme } = useColorScheme();
	const [settings] = useUiSettings();
	const fetcher = useMemo(
		() =>
			createGraphiQLFetcher({
				url: graphQLEndpoint,
				fetch: (input: RequestInfo | URL, init?: RequestInit) =>
					fetch(input, {
						...init,
						credentials: settings.allowAnonymousConsoleRequests ? "omit" : "include",
					}),
			}),
		[settings.allowAnonymousConsoleRequests],
	);

	// GraphiQL v5+ uses CSS classes for theming
	const themeClass = colorScheme === "dark" ? "graphiql-dark" : "graphiql-light";

	return (
		<ToolWorkspaceLayout
			title="GraphQL"
			description="Explore and execute GraphQL queries against the FHIR server"
			maxWidth="none"
		>
			<div
				className={themeClass}
				style={{
					flex: 1,
					minHeight: 560,
					backgroundColor: "var(--octo-surface-1)",
					border: "1px solid var(--g-color-line-generic)",
					borderRadius: 8,
					overflow: "hidden",
				}}
			>
				<GraphiQL
					fetcher={fetcher}
					defaultQuery={DEFAULT_QUERY}
					shouldPersistHeaders
				/>
			</div>
		</ToolWorkspaceLayout>
	);
}

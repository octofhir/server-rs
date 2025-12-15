import { useMemo } from "react";
import { GraphiQL } from "graphiql";
import { createGraphiQLFetcher } from "@graphiql/toolkit";
import "graphiql/style.css";

const DEFAULT_QUERY = `# Welcome to the OctoFHIR GraphQL Console!
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
	const fetcher = useMemo(
		() =>
			createGraphiQLFetcher({
				url: graphQLEndpoint,
				fetch: (input: RequestInfo | URL, init?: RequestInit) =>
					fetch(input, {
						...init,
						credentials: "include",
					}),
			}),
		[],
	);

	return (
		<div style={{ flex: 1, minHeight: 0 }}>
			<GraphiQL
				fetcher={fetcher}
				defaultQuery={DEFAULT_QUERY}
				shouldPersistHeaders
			/>
		</div>
	);
}

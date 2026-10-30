import { useMemo } from "react";
import { GraphiQL } from "graphiql";
import { createGraphiQLFetcher } from "@graphiql/toolkit";
import { useColorScheme } from "@octofhir/ui-kit";
import "graphiql/style.css";
import { useUiSettings } from "@/shared";
import { OCTOFHIR_THEME_DARK, OCTOFHIR_THEME_LIGHT, useOctoMonacoTheme } from "@/shared/monaco/theme";
import classes from "./GraphQLConsolePage.module.css";

const DEFAULT_QUERY = `# ⌘↵ run · ⇧⌘P prettify · ⌃Space complete

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

const EDITOR_THEMES = { light: OCTOFHIR_THEME_LIGHT, dark: OCTOFHIR_THEME_DARK };

export function GraphQLConsolePage() {
	const { colorScheme } = useColorScheme();
	// Registers the shared Monaco themes so GraphiQL's editors can use them.
	useOctoMonacoTheme();
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

	return (
		<div className={classes.root}>
			<div className={classes.console}>
				{/* forcedTheme keeps GraphiQL on the app's scheme (it otherwise follows the OS via a body class). */}
				<GraphiQL
					fetcher={fetcher}
					defaultQuery={DEFAULT_QUERY}
					shouldPersistHeaders
					forcedTheme={colorScheme}
					editorTheme={EDITOR_THEMES}
				/>
			</div>
		</div>
	);
}

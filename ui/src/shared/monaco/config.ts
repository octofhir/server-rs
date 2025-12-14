/**
 * Monaco Editor Configuration
 *
 * This module configures Monaco with language defaults for Vite.
 * Must be imported before any Monaco usage in the application.
 *
 * Used for:
 * - SQL editing (DB Console)
 * - GraphQL queries (GraphQL Console via GraphiQL + monaco-graphql)
 * - JSON editing (REST Console, FHIR resources, GraphiQL variables/headers)
 * - JavaScript (AccessPolicy scripts)
 *
 * Note: Monaco version is pinned to 0.52.2 to match @graphiql/react peer dependency
 * Workers are configured in vite.config.ts via vite-plugin-monaco-editor
 */

import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";

// Configure language defaults
monaco.languages.json.jsonDefaults.setDiagnosticsOptions({
	validate: true,
	allowComments: true,
	schemas: [],
	enableSchemaRequest: false,
});

monaco.languages.typescript.javascriptDefaults.setCompilerOptions({
	target: monaco.languages.typescript.ScriptTarget.ESNext,
	allowNonTsExtensions: true,
	moduleResolution: monaco.languages.typescript.ModuleResolutionKind.NodeJs,
	module: monaco.languages.typescript.ModuleKind.CommonJS,
	noEmit: true,
	esModuleInterop: true,
	allowJs: true,
});

monaco.languages.typescript.javascriptDefaults.setDiagnosticsOptions({
	noSemanticValidation: false,
	noSyntaxValidation: false,
});

// Configure @monaco-editor/react to use the local Monaco instance
// instead of loading from CDN
loader.config({ monaco });

console.log("[Monaco] Configuration complete");

// Export monaco for use elsewhere if needed
export { monaco };

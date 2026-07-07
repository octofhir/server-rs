// Load OctoFHIR ui-kit base styles + design tokens FIRST so the cascade
// is set up before any component-local CSS modules run.
import "@octofhir/ui-kit/styles";

import "./shared/lib/react-dom-legacy-compat";
// Must run before ./app so prismjs' global is set before @lexical/code's
// language modules evaluate. See prism-global.ts.
import "./shared/lib/prism-global";
import { createRoot } from "react-dom/client";
import { App } from "./app";

const rootElement = document.getElementById("root");
if (!rootElement) {
	throw new Error("Root element not found");
}

const root = createRoot(rootElement);
root.render(<App />);

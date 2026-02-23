import type * as Monaco from "monaco-editor";
import { parseQueryAst } from "../../core/parser";
import { computeDiagnostics } from "../../core/diagnostics";
import type { Diagnostic, QueryInputMetadata } from "../../core/types";

const SEVERITY_MAP: Record<string, number> = {
	error: 8, // MarkerSeverity.Error
	warning: 4, // MarkerSeverity.Warning
	info: 2, // MarkerSeverity.Info
};

const MARKER_OWNER = "fhir-query-diagnostics";

export function setDiagnosticMarkers(
	monaco: typeof import("monaco-editor"),
	model: Monaco.editor.ITextModel,
	diagnostics: Diagnostic[],
): void {
	const markers: Monaco.editor.IMarkerData[] = diagnostics.map((d) => ({
		severity: SEVERITY_MAP[d.severity] ?? 2,
		message: d.message,
		startLineNumber: 1,
		startColumn: d.span.start + 1, // Monaco is 1-indexed
		endLineNumber: 1,
		endColumn: d.span.end + 1,
		source: "FHIR Query",
		code: d.code,
	}));

	monaco.editor.setModelMarkers(model, MARKER_OWNER, markers);
}

export function updateDiagnosticsFromContent(
	monaco: typeof import("monaco-editor"),
	model: Monaco.editor.ITextModel,
	metadata: QueryInputMetadata,
	basePath = "/fhir",
): void {
	const content = model.getValue();
	const ast = parseQueryAst(content, basePath);
	const diagnostics = computeDiagnostics(ast, metadata);
	setDiagnosticMarkers(monaco, model, diagnostics);
}

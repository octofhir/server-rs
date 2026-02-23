// Core (framework-agnostic, testable without React)
export { parseQueryAst } from "./core/parser";
export { serializeAst } from "./core/serializer";
export { getCursorContext } from "./core/cursor-context";
export { getSuggestions } from "./core/suggestions";
export { computeDiagnostics } from "./core/diagnostics";
export {
	astToBuilderState,
	builderStateToAst,
	builderStateToRaw,
} from "./core/builder-model";
export type { BuilderState, BuilderParam } from "./core/builder-model";
export type {
	QueryAst,
	PathNode,
	QueryParamNode,
	QueryValueNode,
	TextSpan,
	CursorContext,
	CursorContextType,
	QuerySuggestion,
	QueryInputMetadata,
	Diagnostic,
	DiagnosticSeverity,
	DiagnosticCode,
} from "./core/types";

// React hook (headless, UI-agnostic)
export { useFhirQueryInput } from "./hooks/useFhirQueryInput";
export type {
	UseFhirQueryInputOptions,
	UseFhirQueryInputReturn,
} from "./hooks/useFhirQueryInput";

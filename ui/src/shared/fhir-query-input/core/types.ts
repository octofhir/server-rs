import type {
	AutocompleteSuggestion,
	RestConsoleResponse,
	RestConsoleSearchParam,
} from "@/shared/api";

export type TextSpan = { start: number; end: number };

export type QueryAst = {
	raw: string;
	basePath: string;
	path: PathNode;
	params: QueryParamNode[];
};

export type PathNode =
	| { kind: "resource-type"; resourceType: string; span: TextSpan }
	| {
			kind: "resource-instance";
			resourceType: string;
			id: string;
			span: TextSpan;
	  }
	| {
			kind: "type-operation";
			resourceType: string;
			operation: string;
			span: TextSpan;
	  }
	| {
			kind: "instance-operation";
			resourceType: string;
			id: string;
			operation: string;
			span: TextSpan;
	  }
	| { kind: "system-operation"; operation: string; span: TextSpan }
	| { kind: "api-endpoint"; path: string; span: TextSpan }
	| { kind: "root"; span: TextSpan }
	| { kind: "unknown"; text: string; span: TextSpan };

export type QueryParamNode = {
	name: string;
	modifier?: string;
	values: QueryValueNode[];
	isSpecial: boolean;
	span: TextSpan;
};

export type QueryValueNode = {
	raw: string;
	prefix?: string;
	span: TextSpan;
};

export type CursorContextType =
	| "root"
	| "base-path"
	| "resource-type"
	| "resource-id"
	| "next-after-resource"
	| "next-after-id"
	| "type-operation"
	| "instance-operation"
	| "system-operation"
	| "query-param"
	| "query-modifier"
	| "query-value"
	| "api-endpoint"
	| "unknown";

export type CursorContext = {
	type: CursorContextType;
	resourceType?: string;
	resourceId?: string;
	paramName?: string;
	paramType?: string;
	fragment: string;
	span: TextSpan;
};

export type QuerySuggestion = {
	label: string;
	insertText: string;
	/** Override Monaco's filter text (defaults to label if not set) */
	filterText?: string;
	kind:
		| "resource"
		| "operation"
		| "param"
		| "modifier"
		| "value"
		| "prefix"
		| "special"
		| "api-endpoint"
		| "structural";
	detail?: string;
	documentation?: string;
	sortPriority: number;
};

export type QueryInputMetadata = {
	resourceTypes: string[];
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
	allSuggestions: AutocompleteSuggestion[];
	/** Enriched capabilities (resources, special_params, system_operations) */
	capabilities?: RestConsoleResponse;
};

export type DiagnosticSeverity = "error" | "warning" | "info";

export type DiagnosticCode =
	| "unknown-resource"
	| "unknown-param"
	| "invalid-modifier"
	| "invalid-prefix"
	| "invalid-value"
	| "empty-param-name"
	| "empty-value"
	| "duplicate-param";

export type Diagnostic = {
	severity: DiagnosticSeverity;
	message: string;
	span: TextSpan;
	code?: DiagnosticCode;
};

import type { Diagnostic, QueryAst, QueryInputMetadata, QueryParamNode } from "./types";

const VALID_SUMMARY_VALUES = new Set(["true", "false", "count", "text", "data"]);
const VALID_TOTAL_VALUES = new Set(["none", "estimate", "accurate"]);
const PREFIX_TYPES = new Set(["date", "number", "quantity"]);

export function computeDiagnostics(
	ast: QueryAst,
	metadata: QueryInputMetadata,
): Diagnostic[] {
	const diagnostics: Diagnostic[] = [];

	// Path-level diagnostics
	checkPath(ast, metadata, diagnostics);

	// Query param diagnostics
	checkParams(ast, metadata, diagnostics);

	return diagnostics;
}

function checkPath(
	ast: QueryAst,
	metadata: QueryInputMetadata,
	diagnostics: Diagnostic[],
): void {
	const { path } = ast;

	if (path.kind === "resource-type" || path.kind === "resource-instance" ||
		path.kind === "type-operation" || path.kind === "instance-operation") {
		const rt = path.resourceType;
		if (rt && metadata.resourceTypes.length > 0 && !metadata.resourceTypes.includes(rt)) {
			diagnostics.push({
				severity: "error",
				message: `Unknown resource type '${rt}'`,
				span: path.span,
				code: "unknown-resource",
			});
		}
	}
}

function checkParams(
	ast: QueryAst,
	metadata: QueryInputMetadata,
	diagnostics: Diagnostic[],
): void {
	const resourceType = getResourceType(ast);
	const seen = new Map<string, QueryParamNode>();

	for (const param of ast.params) {
		// Empty param name
		if (!param.name) {
			diagnostics.push({
				severity: "error",
				message: "Empty parameter name",
				span: param.span,
				code: "empty-param-name",
			});
			continue;
		}

		// Duplicate param detection (skip special params that can repeat like _include)
		const repeatableParams = new Set(["_include", "_revinclude", "_has", "_sort"]);
		if (!repeatableParams.has(param.name)) {
			const prev = seen.get(param.name);
			if (prev) {
				diagnostics.push({
					severity: "warning",
					message: `Duplicate parameter '${param.name}'`,
					span: param.span,
					code: "duplicate-param",
				});
			}
		}
		seen.set(param.name, param);

		// Special params validation
		if (param.isSpecial) {
			checkSpecialParam(param, resourceType, metadata, diagnostics);
			continue;
		}

		// Regular param validation (requires resource type and metadata)
		if (resourceType) {
			checkRegularParam(param, resourceType, metadata, diagnostics);
		}
	}
}

function checkSpecialParam(
	param: QueryParamNode,
	resourceType: string | undefined,
	metadata: QueryInputMetadata,
	diagnostics: Diagnostic[],
): void {
	switch (param.name) {
		case "_count": {
			for (const v of param.values) {
				if (v.raw && (!/^\d+$/.test(v.raw) || Number.parseInt(v.raw) <= 0)) {
					diagnostics.push({
						severity: "error",
						message: `_count must be a positive integer, got '${v.raw}'`,
						span: v.span,
						code: "invalid-value",
					});
				}
			}
			break;
		}
		case "_offset": {
			for (const v of param.values) {
				if (v.raw && !/^\d+$/.test(v.raw)) {
					diagnostics.push({
						severity: "error",
						message: `_offset must be a non-negative integer, got '${v.raw}'`,
						span: v.span,
						code: "invalid-value",
					});
				}
			}
			break;
		}
		case "_summary": {
			for (const v of param.values) {
				if (v.raw && !VALID_SUMMARY_VALUES.has(v.raw)) {
					diagnostics.push({
						severity: "error",
						message: `Invalid _summary value '${v.raw}'. Expected: true, false, count, text, data`,
						span: v.span,
						code: "invalid-value",
					});
				}
			}
			break;
		}
		case "_total": {
			for (const v of param.values) {
				if (v.raw && !VALID_TOTAL_VALUES.has(v.raw)) {
					diagnostics.push({
						severity: "error",
						message: `Invalid _total value '${v.raw}'. Expected: none, estimate, accurate`,
						span: v.span,
						code: "invalid-value",
					});
				}
			}
			break;
		}
		case "_sort": {
			if (!resourceType || !metadata.capabilities) break;
			const resCap = metadata.capabilities.resources.find(
				(r) => r.resource_type === resourceType,
			);
			if (!resCap) break;
			for (const v of param.values) {
				const sortField = v.raw.startsWith("-") ? v.raw.slice(1) : v.raw;
				if (sortField && !resCap.sort_params.includes(sortField)) {
					diagnostics.push({
						severity: "warning",
						message: `Unknown sort parameter '${sortField}' for ${resourceType}`,
						span: v.span,
						code: "invalid-value",
					});
				}
			}
			break;
		}
		case "_include":
		case "_revinclude": {
			if (!resourceType || !metadata.capabilities) break;
			const resCap = metadata.capabilities.resources.find(
				(r) => r.resource_type === resourceType,
			);
			if (!resCap) break;
			const isRev = param.name === "_revinclude";
			const source = isRev ? resCap.rev_includes : resCap.includes;
			const validValues = new Set<string>();
			for (const inc of source) {
				for (const target of inc.target_types) {
					if (isRev) {
						validValues.add(inc.param_code);
					} else {
						validValues.add(`${resCap.resource_type}:${inc.param_code}:${target}`);
					}
				}
			}
			for (const v of param.values) {
				if (v.raw && v.raw !== "*" && !validValues.has(v.raw)) {
					diagnostics.push({
						severity: "warning",
						message: `Unknown ${param.name} value '${v.raw}'`,
						span: v.span,
						code: "invalid-value",
					});
				}
			}
			break;
		}
	}
}

function checkRegularParam(
	param: QueryParamNode,
	resourceType: string,
	metadata: QueryInputMetadata,
	diagnostics: Diagnostic[],
): void {
	const params = metadata.searchParamsByResource[resourceType];
	if (!params) return;

	const paramDef = params.find((p) => p.code === param.name);
	if (!paramDef) {
		diagnostics.push({
			severity: "error",
			message: `Unknown search parameter '${param.name}' for ${resourceType}`,
			span: param.span,
			code: "unknown-param",
		});
		return;
	}

	// Check modifier validity
	if (param.modifier) {
		const validModifiers = paramDef.modifiers?.map((m) => m.code) ?? [];
		if (validModifiers.length > 0 && !validModifiers.includes(param.modifier)) {
			diagnostics.push({
				severity: "warning",
				message: `Unsupported modifier ':${param.modifier}' for parameter '${param.name}'. Valid: ${validModifiers.join(", ")}`,
				span: param.span,
				code: "invalid-modifier",
			});
		}
	}

	// Check prefix validity — prefixes only valid on date, number, quantity
	const paramType = paramDef.type.toLowerCase();
	for (const v of param.values) {
		if (v.prefix && !PREFIX_TYPES.has(paramType)) {
			diagnostics.push({
				severity: "error",
				message: `Search prefix '${v.prefix}' is not valid for ${paramType} parameter '${param.name}'. Prefixes only apply to date, number, quantity types`,
				span: v.span,
				code: "invalid-prefix",
			});
		}
	}
}

function getResourceType(ast: QueryAst): string | undefined {
	const { path } = ast;
	switch (path.kind) {
		case "resource-type":
		case "resource-instance":
		case "type-operation":
		case "instance-operation":
			return path.resourceType;
		default:
			return undefined;
	}
}

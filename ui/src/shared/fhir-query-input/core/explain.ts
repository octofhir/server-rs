import type { QueryAst, QueryInputMetadata } from "./types";

export interface ExplainItem {
	label: string;
	description: string;
	kind: "path" | "param" | "special" | "modifier" | "value";
}

const PREFIX_LABELS: Record<string, string> = {
	eq: "equal to",
	ne: "not equal to",
	gt: "greater than",
	lt: "less than",
	ge: "on or after",
	le: "on or before",
	sa: "starting after",
	eb: "ending before",
	ap: "approximately",
};

export function explainQuery(
	ast: QueryAst,
	metadata: QueryInputMetadata,
): ExplainItem[] {
	const items: ExplainItem[] = [];

	explainPath(ast, items);
	explainParams(ast, metadata, items);

	return items;
}

function explainPath(ast: QueryAst, items: ExplainItem[]): void {
	const { path } = ast;

	switch (path.kind) {
		case "resource-type":
			items.push({
				label: path.resourceType,
				description: `Search ${path.resourceType} resources`,
				kind: "path",
			});
			break;
		case "resource-instance":
			items.push({
				label: `${path.resourceType}/${path.id}`,
				description: `Read ${path.resourceType} with ID '${path.id}'`,
				kind: "path",
			});
			break;
		case "type-operation":
			items.push({
				label: `${path.resourceType}/${path.operation}`,
				description: `Execute ${path.operation} on ${path.resourceType} type`,
				kind: "path",
			});
			break;
		case "instance-operation":
			items.push({
				label: `${path.resourceType}/${path.id}/${path.operation}`,
				description: `Execute ${path.operation} on ${path.resourceType}/${path.id}`,
				kind: "path",
			});
			break;
		case "system-operation":
			items.push({
				label: path.operation,
				description: `Execute system-level operation ${path.operation}`,
				kind: "path",
			});
			break;
		case "api-endpoint":
			items.push({
				label: path.path,
				description: `Internal API endpoint: ${path.path}`,
				kind: "path",
			});
			break;
	}
}

function explainParams(
	ast: QueryAst,
	metadata: QueryInputMetadata,
	items: ExplainItem[],
): void {
	const resourceType = getResourceType(ast);

	for (const param of ast.params) {
		if (param.isSpecial) {
			explainSpecialParam(param, items);
		} else {
			explainSearchParam(param, resourceType, metadata, items);
		}
	}
}

function explainSpecialParam(
	param: { name: string; values: Array<{ raw: string; prefix?: string }> },
	items: ExplainItem[],
): void {
	const valueStr = param.values.map((v) => v.raw).join(", ");

	switch (param.name) {
		case "_count":
			items.push({
				label: `_count=${valueStr}`,
				description: `Return at most ${valueStr} results per page`,
				kind: "special",
			});
			break;
		case "_offset":
			items.push({
				label: `_offset=${valueStr}`,
				description: `Skip the first ${valueStr} results`,
				kind: "special",
			});
			break;
		case "_sort":
			items.push({
				label: `_sort=${valueStr}`,
				description: explainSort(valueStr),
				kind: "special",
			});
			break;
		case "_summary":
			items.push({
				label: `_summary=${valueStr}`,
				description: explainSummary(valueStr),
				kind: "special",
			});
			break;
		case "_total":
			items.push({
				label: `_total=${valueStr}`,
				description: `Request '${valueStr}' total count mode`,
				kind: "special",
			});
			break;
		case "_elements":
			items.push({
				label: `_elements=${valueStr}`,
				description: `Include only elements: ${valueStr}`,
				kind: "special",
			});
			break;
		case "_include":
			items.push({
				label: `_include=${valueStr}`,
				description: `Also return referenced resource: ${valueStr}`,
				kind: "special",
			});
			break;
		case "_revinclude":
			items.push({
				label: `_revinclude=${valueStr}`,
				description: `Also return resources that reference this via: ${valueStr}`,
				kind: "special",
			});
			break;
		default:
			items.push({
				label: `${param.name}=${valueStr}`,
				description: `Special parameter ${param.name} = ${valueStr}`,
				kind: "special",
			});
	}
}

function explainSort(value: string): string {
	const parts = value.split(",").map((p) => {
		const trimmed = p.trim();
		if (trimmed.startsWith("-")) {
			return `${trimmed.slice(1)} descending`;
		}
		return `${trimmed} ascending`;
	});
	return `Sort results by ${parts.join(", then ")}`;
}

function explainSummary(value: string): string {
	switch (value) {
		case "true":
			return "Return only summary elements (id, meta, tag)";
		case "false":
			return "Return full resources (no summary)";
		case "count":
			return "Return only the total count, no resources";
		case "text":
			return "Return text summary plus id, meta, top-level mandatory elements";
		case "data":
			return "Return data elements only, remove text narrative";
		default:
			return `Summary mode: ${value}`;
	}
}

function explainSearchParam(
	param: {
		name: string;
		modifier?: string;
		values: Array<{ raw: string; prefix?: string }>;
	},
	resourceType: string | undefined,
	metadata: QueryInputMetadata,
	items: ExplainItem[],
): void {
	const paramName = param.modifier
		? `${param.name}:${param.modifier}`
		: param.name;

	const valueDescriptions = param.values.map((v) => {
		if (v.prefix) {
			const prefixLabel = PREFIX_LABELS[v.prefix] ?? v.prefix;
			const actualValue = v.raw.slice(2);
			return `${prefixLabel} ${actualValue}`;
		}
		return `'${v.raw}'`;
	});

	const valuesJoined =
		valueDescriptions.length > 1
			? valueDescriptions.join(" OR ")
			: valueDescriptions[0] ?? "";

	let modifierExplain = "";
	if (param.modifier) {
		modifierExplain = explainModifier(param.modifier);
	}

	const valueStr = param.values.map((v) => v.raw).join(",");
	const label = `${paramName}=${valueStr}`;

	// Get param type if available
	let paramType = "";
	if (resourceType) {
		const params = metadata.searchParamsByResource[resourceType] ?? [];
		const def = params.find((p) => p.code === param.name);
		if (def) paramType = def.type;
	}

	let description: string;
	if (modifierExplain) {
		description = `Where '${param.name}' ${modifierExplain} ${valuesJoined}`;
	} else if (paramType === "date" && param.values[0]?.prefix) {
		description = `Where '${param.name}' is ${valuesJoined}`;
	} else {
		description = `Where '${param.name}' matches ${valuesJoined}`;
	}

	items.push({ label, description, kind: "param" });
}

function explainModifier(modifier: string): string {
	switch (modifier) {
		case "exact":
			return "is exactly";
		case "contains":
			return "contains";
		case "text":
			return "text search matches";
		case "not":
			return "does not match";
		case "above":
			return "is above (in hierarchy)";
		case "below":
			return "is below (in hierarchy)";
		case "in":
			return "is in value set";
		case "not-in":
			return "is not in value set";
		case "missing":
			return "is missing:";
		case "type":
			return "has type";
		case "identifier":
			return "has identifier matching";
		case "of-type":
			return "has type matching";
		default:
			return `with modifier :${modifier} matches`;
	}
}

function getResourceType(ast: QueryAst): string | undefined {
	switch (ast.path.kind) {
		case "resource-type":
		case "resource-instance":
		case "type-operation":
		case "instance-operation":
			return ast.path.resourceType;
		default:
			return undefined;
	}
}

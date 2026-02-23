import type { QueryAst } from "./types";
import { parseQueryAst } from "./parser";
import { serializeAst } from "./serializer";

export interface BuilderState {
	resourceType?: string;
	resourceId?: string;
	operation?: string;
	params: BuilderParam[];
}

export interface BuilderParam {
	id: string;
	code: string;
	modifier?: string;
	value: string;
	isSpecial: boolean;
}

let nextId = 1;

function genId(): string {
	return `bp_${nextId++}`;
}

export function astToBuilderState(ast: QueryAst): BuilderState {
	const state: BuilderState = {
		params: [],
	};

	// Extract path info
	switch (ast.path.kind) {
		case "resource-type":
			state.resourceType = ast.path.resourceType;
			break;
		case "resource-instance":
			state.resourceType = ast.path.resourceType;
			state.resourceId = ast.path.id;
			break;
		case "type-operation":
			state.resourceType = ast.path.resourceType;
			state.operation = ast.path.operation;
			break;
		case "instance-operation":
			state.resourceType = ast.path.resourceType;
			state.resourceId = ast.path.id;
			state.operation = ast.path.operation;
			break;
		case "system-operation":
			state.operation = ast.path.operation;
			break;
	}

	// Extract params
	for (const param of ast.params) {
		const value = param.values.map((v) => v.raw).join(",");
		state.params.push({
			id: genId(),
			code: param.name,
			modifier: param.modifier,
			value,
			isSpecial: param.isSpecial,
		});
	}

	return state;
}

export function builderStateToAst(
	state: BuilderState,
	basePath = "/fhir",
): QueryAst {
	// Build raw URL from state
	let pathPart = basePath;

	if (state.resourceType) {
		pathPart += `/${state.resourceType}`;
		if (state.resourceId) {
			pathPart += `/${state.resourceId}`;
		}
		if (state.operation) {
			pathPart += `/${state.operation}`;
		}
	} else if (state.operation) {
		pathPart += `/${state.operation}`;
	}

	// Build query string
	const queryTokens: string[] = [];
	for (const param of state.params) {
		if (!param.code) continue;
		const key = param.modifier ? `${param.code}:${param.modifier}` : param.code;
		queryTokens.push(`${key}=${param.value}`);
	}

	const raw =
		queryTokens.length > 0
			? `${pathPart}?${queryTokens.join("&")}`
			: pathPart;

	return parseQueryAst(raw, basePath);
}

export function builderStateToRaw(
	state: BuilderState,
	basePath = "/fhir",
): string {
	const ast = builderStateToAst(state, basePath);
	return serializeAst(ast);
}

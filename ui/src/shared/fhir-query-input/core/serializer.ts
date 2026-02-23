import type { QueryAst } from "./types";

export function serializeAst(ast: QueryAst): string {
	let result = serializePath(ast);

	if (ast.params.length > 0) {
		const queryStr = ast.params
			.map((param) => {
				const modifier = param.modifier ? `:${param.modifier}` : "";
				const key = `${param.name}${modifier}`;
				const value = param.values.map((v) => v.raw).join(",");
				if (value) {
					return `${key}=${value}`;
				}
				return key;
			})
			.join("&");
		result += `?${queryStr}`;
	}

	return result;
}

function serializePath(ast: QueryAst): string {
	const { path, basePath } = ast;

	switch (path.kind) {
		case "root":
			return basePath;
		case "api-endpoint":
			return path.path;
		case "resource-type":
			return `${basePath}/${path.resourceType}`;
		case "resource-instance":
			return `${basePath}/${path.resourceType}/${path.id}`;
		case "type-operation":
			return `${basePath}/${path.resourceType}/${path.operation}`;
		case "instance-operation":
			return `${basePath}/${path.resourceType}/${path.id}/${path.operation}`;
		case "system-operation":
			return `${basePath}/${path.operation}`;
		case "unknown":
			return path.text;
	}
}

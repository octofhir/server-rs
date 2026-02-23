import type * as Monaco from "monaco-editor";
import { parseQueryAst } from "../../core/parser";
import type { QueryInputMetadata } from "../../core/types";
import { LANGUAGE_ID } from "./language";

const PREFIX_DESCRIPTIONS: Record<string, string> = {
	eq: "**eq** — Equal to",
	ne: "**ne** — Not equal to",
	gt: "**gt** — Greater than",
	lt: "**lt** — Less than",
	ge: "**ge** — Greater than or equal to",
	le: "**le** — Less than or equal to",
	sa: "**sa** — Starts after",
	eb: "**eb** — Ends before",
	ap: "**ap** — Approximately",
};

export function createHoverProvider(
	getMetadata: () => QueryInputMetadata,
	basePath = "/fhir",
): Monaco.languages.HoverProvider {
	return {
		provideHover(
			model: Monaco.editor.ITextModel,
			position: Monaco.Position,
		): Monaco.languages.Hover | null {
			const lineContent = model.getLineContent(position.lineNumber);
			const col = position.column - 1;

			const ast = parseQueryAst(lineContent, basePath);
			const metadata = getMetadata();

			// Check if hovering over a resource type
			if (
				(ast.path.kind === "resource-type" ||
					ast.path.kind === "resource-instance" ||
					ast.path.kind === "type-operation" ||
					ast.path.kind === "instance-operation") &&
				isInSpan(col, getResourceTypeSpanInLine(lineContent, ast.path.resourceType, basePath))
			) {
				const rt = ast.path.resourceType;
				const resCap = metadata.capabilities?.resources.find(
					(r) => r.resource_type === rt,
				);
				const paramCount = metadata.searchParamsByResource[rt]?.length ?? 0;
				const lines = [`**${rt}** — FHIR Resource Type`];
				if (paramCount > 0) lines.push(`${paramCount} search parameters`);
				if (resCap) {
					lines.push(`${resCap.sort_params.length} sortable fields`);
					lines.push(`${resCap.includes.length} _include paths`);
				}
				return {
					contents: [{ value: lines.join("\n\n") }],
				};
			}

			// Check if hovering over a query param
			for (const param of ast.params) {
				if (!isInSpan(col, param.span)) continue;

				const resourceType = getAstResourceType(ast);

				// Hovering over param name
				if (param.isSpecial) {
					const sp = metadata.capabilities?.special_params.find(
						(s) => s.name === param.name,
					);
					if (sp) {
						const lines = [`**${sp.name}** — Special Parameter`];
						if (sp.description) lines.push(sp.description);
						if (sp.examples.length > 0)
							lines.push(`Examples: \`${sp.examples.join("`, `")}\``);
						return { contents: [{ value: lines.join("\n\n") }] };
					}
				} else if (resourceType) {
					const params = metadata.searchParamsByResource[resourceType] ?? [];
					const paramDef = params.find((p) => p.code === param.name);
					if (paramDef) {
						const lines = [
							`**${paramDef.code}** — \`${paramDef.type}\` search parameter`,
						];
						if (paramDef.description) lines.push(paramDef.description);
						if (paramDef.modifiers?.length) {
							lines.push(
								`Modifiers: ${paramDef.modifiers.map((m) => `\`:${m.code}\``).join(", ")}`,
							);
						}
						if (paramDef.comparators?.length) {
							lines.push(
								`Comparators: ${paramDef.comparators.join(", ")}`,
							);
						}
						if (paramDef.targets?.length) {
							lines.push(`Targets: ${paramDef.targets.join(", ")}`);
						}
						return { contents: [{ value: lines.join("\n\n") }] };
					}
				}

				// Hovering over a value with prefix
				for (const v of param.values) {
					if (v.prefix && isInSpan(col, v.span)) {
						const desc = PREFIX_DESCRIPTIONS[v.prefix];
						if (desc) {
							return { contents: [{ value: desc }] };
						}
					}
				}
			}

			return null;
		},
	};
}

function isInSpan(
	col: number,
	span: { start: number; end: number } | null,
): boolean {
	if (!span) return false;
	return col >= span.start && col < span.end;
}

function getResourceTypeSpanInLine(
	line: string,
	resourceType: string,
	basePath: string,
): { start: number; end: number } | null {
	const baseIdx = line.indexOf(basePath);
	if (baseIdx === -1) return null;
	const rtIdx = line.indexOf(resourceType, baseIdx + basePath.length);
	if (rtIdx === -1) return null;
	return { start: rtIdx, end: rtIdx + resourceType.length };
}

function getAstResourceType(
	ast: ReturnType<typeof parseQueryAst>,
): string | undefined {
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

export function registerHoverProvider(
	monaco: typeof import("monaco-editor"),
	getMetadata: () => QueryInputMetadata,
	basePath?: string,
): Monaco.IDisposable {
	return monaco.languages.registerHoverProvider(
		LANGUAGE_ID,
		createHoverProvider(getMetadata, basePath),
	);
}

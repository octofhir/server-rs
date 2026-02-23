import { describe, it, expect } from "vitest";
import { parseQueryAst } from "./parser";
import { serializeAst } from "./serializer";
import { getCursorContext } from "./cursor-context";
import { getSuggestions } from "./suggestions";
import type { QueryInputMetadata } from "./types";

/**
 * Integration test: verifies parse -> getCursorContext -> getSuggestions chain
 * works with realistic metadata, proving reusability outside RestConsole.
 */
describe("fhir-query-input integration", () => {
	const metadata: QueryInputMetadata = {
		resourceTypes: ["Patient", "Observation", "Condition"],
		searchParamsByResource: {
			Patient: [
				{
					code: "name",
					type: "string",
					description: "Patient name",
					modifiers: [
						{ code: "exact", description: "Exact match" },
						{ code: "contains", description: "Contains match" },
					],
					comparators: [],
					targets: [],
					is_common: false,
				},
				{
					code: "birthdate",
					type: "date",
					description: "Patient birth date",
					modifiers: [],
					comparators: ["eq", "ne", "gt", "lt", "ge", "le"],
					targets: [],
					is_common: false,
				},
				{
					code: "_id",
					type: "token",
					description: "Resource ID",
					modifiers: [],
					comparators: [],
					targets: [],
					is_common: true,
				},
			],
			Observation: [
				{
					code: "code",
					type: "token",
					description: "Observation code",
					modifiers: [
						{ code: "text" },
						{ code: "in" },
					],
					comparators: [],
					targets: [],
					is_common: false,
				},
				{
					code: "subject",
					type: "reference",
					description: "Subject reference",
					modifiers: [],
					comparators: [],
					targets: ["Patient"],
					is_common: false,
				},
			],
		},
		allSuggestions: [
			{
				id: "Patient",
				kind: "resource",
				label: "Patient",
				path_template: "/fhir/Patient",
				methods: ["GET", "POST"],
				placeholders: [],
				description: "Patient resource",
				metadata: { affects_state: false, requires_body: false },
			},
			{
				id: "Observation",
				kind: "resource",
				label: "Observation",
				path_template: "/fhir/Observation",
				methods: ["GET", "POST"],
				placeholders: [],
				description: "Observation resource",
				metadata: { affects_state: false, requires_body: false },
			},
			{
				id: "Condition",
				kind: "resource",
				label: "Condition",
				path_template: "/fhir/Condition",
				methods: ["GET", "POST"],
				placeholders: [],
				description: "Condition resource",
				metadata: { affects_state: false, requires_body: false },
			},
			{
				id: "$validate-Patient",
				kind: "type-op",
				label: "/$validate",
				path_template: "/fhir/{resourceType}/$validate",
				methods: ["POST"],
				placeholders: [],
				description: "Validate a resource",
				metadata: {
					resource_type: "Patient",
					affects_state: false,
					requires_body: true,
				},
			},
			{
				id: "$export",
				kind: "system-op",
				label: "/$export",
				path_template: "/fhir/$export",
				methods: ["GET"],
				placeholders: [],
				description: "Bulk data export",
				metadata: { affects_state: false, requires_body: false },
			},
		],
	};

	it("suggests resource types when typing after /fhir/", () => {
		const raw = "/fhir/Pat";
		const ctx = getCursorContext(raw, raw.length, metadata.resourceTypes);
		const suggestions = getSuggestions(ctx, metadata);
		expect(suggestions.some((s) => s.label === "Patient")).toBe(true);
		expect(suggestions.some((s) => s.label === "Observation")).toBe(false);
	});

	it("suggests next steps after complete resource type", () => {
		const raw = "/fhir/Patient";
		const ctx = getCursorContext(raw, raw.length, metadata.resourceTypes);
		expect(ctx.type).toBe("next-after-resource");
		const suggestions = getSuggestions(ctx, metadata);
		expect(suggestions.some((s) => s.label === "/{id}")).toBe(true);
		expect(suggestions.some((s) => s.label === "?")).toBe(true);
		expect(suggestions.some((s) => s.label === "/$validate")).toBe(true);
	});

	it("suggests search params after ?", () => {
		const raw = "/fhir/Patient?";
		const ctx = getCursorContext(raw, raw.length, metadata.resourceTypes);
		expect(ctx.type).toBe("query-param");
		const suggestions = getSuggestions(ctx, metadata);
		expect(suggestions.some((s) => s.label === "name")).toBe(true);
		expect(suggestions.some((s) => s.label === "birthdate")).toBe(true);
	});

	it("suggests modifiers after colon", () => {
		const raw = "/fhir/Patient?name:";
		const ctx = getCursorContext(raw, raw.length, metadata.resourceTypes);
		expect(ctx.type).toBe("query-modifier");
		const suggestions = getSuggestions(ctx, metadata);
		expect(suggestions.some((s) => s.label === ":exact")).toBe(true);
		expect(suggestions.some((s) => s.label === ":contains")).toBe(true);
	});

	it("parse -> serialize round-trip preserves URL", () => {
		const urls = [
			"/fhir/Patient",
			"/fhir/Patient/abc-123",
			"/fhir/Patient/$validate",
			"/fhir/Patient?name=John&birthdate=ge2000-01-01",
			"/fhir/Patient?name:exact=John",
			"/fhir/$export",
		];
		for (const url of urls) {
			const ast = parseQueryAst(url);
			expect(serializeAst(ast)).toBe(url);
		}
	});

	it("full chain: parse AST has correct structure", () => {
		const raw = "/fhir/Patient?name:exact=John&birthdate=ge2000-01-01";
		const ast = parseQueryAst(raw);

		expect(ast.path.kind).toBe("resource-type");
		if (ast.path.kind === "resource-type") {
			expect(ast.path.resourceType).toBe("Patient");
		}

		expect(ast.params).toHaveLength(2);
		expect(ast.params[0].name).toBe("name");
		expect(ast.params[0].modifier).toBe("exact");
		expect(ast.params[0].values[0].raw).toBe("John");

		expect(ast.params[1].name).toBe("birthdate");
		expect(ast.params[1].values[0].prefix).toBe("ge");
	});

	it("suggests system operations in resource-type context", () => {
		const raw = "/fhir/$";
		const ctx = getCursorContext(raw, raw.length, metadata.resourceTypes);
		const suggestions = getSuggestions(ctx, metadata);
		expect(suggestions.some((s) => s.label === "/$export")).toBe(true);
	});

	// --- Insertion correctness tests ---
	// These simulate what Monaco does: replace the fragment range with insertText

	/**
	 * Helper: simulates Monaco insertion by replacing characters in `raw`
	 * from `replaceStart` to `replaceEnd` (0-based offsets) with `insertText`.
	 */
	function simulateInsert(raw: string, replaceStart: number, replaceEnd: number, insertText: string): string {
		return raw.slice(0, replaceStart) + insertText + raw.slice(replaceEnd);
	}

	/**
	 * Helper: given a raw string and cursor at end, get the suggestion and compute
	 * what the string looks like after insertion.
	 */
	function getInsertionResult(raw: string, findLabel: string): string | null {
		const ctx = getCursorContext(raw, raw.length, metadata.resourceTypes);
		const suggestions = getSuggestions(ctx, metadata);
		const suggestion = suggestions.find((s) => s.label === findLabel);
		if (!suggestion) return null;

		// Simulate getFragmentRange logic (0-based offsets)
		const before = raw.slice(0, raw.length);
		let replaceStart: number;
		let replaceEnd = raw.length;

		switch (ctx.type) {
			case "query-param": {
				const sep = Math.max(before.lastIndexOf("&"), before.lastIndexOf("?"));
				replaceStart = sep + 1;
				break;
			}
			case "query-modifier": {
				replaceStart = before.lastIndexOf(":") + 1;
				break;
			}
			case "query-value": {
				replaceStart = before.lastIndexOf("=") + 1;
				break;
			}
			case "resource-type":
			case "type-operation":
			case "system-operation":
			case "instance-operation": {
				replaceStart = before.lastIndexOf("/") + 1;
				break;
			}
			case "next-after-resource":
			case "next-after-id": {
				replaceStart = raw.length; // zero-width
				break;
			}
			default:
				replaceStart = 0;
		}

		return simulateInsert(raw, replaceStart, replaceEnd, suggestion.insertText);
	}

	it("modifier insertion preserves parameter name", () => {
		const result = getInsertionResult("/fhir/Patient?name:ex", ":exact");
		expect(result).toBe("/fhir/Patient?name:exact=");
	});

	it("modifier insertion with partial modifier preserves param", () => {
		const result = getInsertionResult("/fhir/Patient?name:con", ":contains");
		expect(result).toBe("/fhir/Patient?name:contains=");
	});

	it("system operation does not create double slash", () => {
		const result = getInsertionResult("/fhir/$", "/$export");
		expect(result).toBe("/fhir/$export");
	});

	it("system operation partial does not create double slash", () => {
		const result = getInsertionResult("/fhir/$exp", "/$export");
		expect(result).toBe("/fhir/$export");
	});

	it("type operation inserted after resource type", () => {
		const result = getInsertionResult("/fhir/Patient", "/$validate");
		expect(result).toBe("/fhir/Patient/$validate");
	});

	it("resource type selection replaces fragment", () => {
		const result = getInsertionResult("/fhir/Pat", "Patient");
		expect(result).toBe("/fhir/Patient");
	});

	it("query param insertion after ?", () => {
		const result = getInsertionResult("/fhir/Patient?nam", "name");
		expect(result).toBe("/fhir/Patient?name=");
	});

	it("query param insertion after &", () => {
		const result = getInsertionResult("/fhir/Patient?name=John&birth", "birthdate");
		expect(result).toBe("/fhir/Patient?name=John&birthdate=");
	});
});

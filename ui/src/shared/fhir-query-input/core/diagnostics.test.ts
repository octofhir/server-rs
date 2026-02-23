import { describe, it, expect } from "vitest";
import { computeDiagnostics } from "./diagnostics";
import { parseQueryAst } from "./parser";
import type { QueryInputMetadata } from "./types";

function makeMeta(overrides?: Partial<QueryInputMetadata>): QueryInputMetadata {
	return {
		resourceTypes: ["Patient", "Observation", "Encounter"],
		searchParamsByResource: {
			Patient: [
				{
					code: "name",
					type: "string",
					modifiers: [
						{ code: "exact" },
						{ code: "contains" },
					],
					comparators: [],
					targets: [],
					is_common: true,
				},
				{
					code: "birthdate",
					type: "date",
					modifiers: [],
					comparators: ["eq", "ne", "gt", "lt", "ge", "le"],
					targets: [],
					is_common: true,
				},
				{
					code: "general-practitioner",
					type: "reference",
					modifiers: [],
					comparators: [],
					targets: ["Practitioner", "Organization"],
					is_common: false,
				},
				{
					code: "active",
					type: "token",
					modifiers: [{ code: "not" }],
					comparators: [],
					targets: [],
					is_common: false,
				},
			],
			Observation: [
				{
					code: "code",
					type: "token",
					modifiers: [{ code: "text" }, { code: "in" }],
					comparators: [],
					targets: [],
					is_common: true,
				},
				{
					code: "value-quantity",
					type: "quantity",
					modifiers: [],
					comparators: ["eq", "ne", "gt", "lt", "ge", "le"],
					targets: [],
					is_common: false,
				},
			],
		},
		allSuggestions: [],
		capabilities: {
			schema_version: 3,
			fhir_version: "4.3.0",
			base_path: "/fhir",
			generated_at: "",
			suggestions: { resources: [], system_operations: [], type_operations: [], instance_operations: [], api_endpoints: [] },
			search_params: {},
			resources: [
				{
					resource_type: "Patient",
					search_params: [],
					includes: [
						{ param_code: "general-practitioner", target_types: ["Practitioner", "Organization"] },
					],
					rev_includes: [
						{ param_code: "Observation:subject", target_types: ["Patient"] },
					],
					sort_params: ["_id", "_lastUpdated", "name", "birthdate"],
					type_operations: [],
					instance_operations: [],
				},
			],
			system_operations: [],
			special_params: [
				{ name: "_count", description: "Max results", supported: true, examples: ["10", "50"] },
				{ name: "_summary", description: "Summary mode", supported: true, examples: [] },
				{ name: "_total", description: "Total count mode", supported: true, examples: [] },
				{ name: "_sort", description: "Sort by", supported: true, examples: [] },
				{ name: "_include", description: "Include refs", supported: true, examples: [] },
				{ name: "_revinclude", description: "Reverse include", supported: true, examples: [] },
			],
		},
		...overrides,
	};
}

function diagnose(raw: string, meta?: Partial<QueryInputMetadata>) {
	const ast = parseQueryAst(raw);
	return computeDiagnostics(ast, makeMeta(meta));
}

describe("computeDiagnostics", () => {
	describe("path diagnostics", () => {
		it("reports unknown resource type", () => {
			const result = diagnose("/fhir/FakeResource?name=test");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "error",
					code: "unknown-resource",
					message: expect.stringContaining("FakeResource"),
				}),
			);
		});

		it("no error for valid resource type", () => {
			const result = diagnose("/fhir/Patient?name=test");
			expect(result.filter((d) => d.code === "unknown-resource")).toHaveLength(0);
		});

		it("no error when resourceTypes list is empty (no metadata loaded yet)", () => {
			const result = diagnose("/fhir/Anything?name=test", { resourceTypes: [] });
			expect(result.filter((d) => d.code === "unknown-resource")).toHaveLength(0);
		});
	});

	describe("unknown param", () => {
		it("reports unknown search parameter", () => {
			const result = diagnose("/fhir/Patient?xyz=1");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "error",
					code: "unknown-param",
					message: expect.stringContaining("xyz"),
				}),
			);
		});

		it("no error for valid search parameter", () => {
			const result = diagnose("/fhir/Patient?name=John");
			expect(result.filter((d) => d.code === "unknown-param")).toHaveLength(0);
		});

		it("no error for special parameters", () => {
			const result = diagnose("/fhir/Patient?_count=10");
			expect(result.filter((d) => d.code === "unknown-param")).toHaveLength(0);
		});
	});

	describe("invalid modifier", () => {
		it("reports unsupported modifier", () => {
			const result = diagnose("/fhir/Patient?name:in=John");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "warning",
					code: "invalid-modifier",
					message: expect.stringContaining(":in"),
				}),
			);
		});

		it("no warning for valid modifier", () => {
			const result = diagnose("/fhir/Patient?name:exact=John");
			expect(result.filter((d) => d.code === "invalid-modifier")).toHaveLength(0);
		});
	});

	describe("invalid prefix", () => {
		it("reports prefix on string parameter", () => {
			const result = diagnose("/fhir/Patient?name=ge2020");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "error",
					code: "invalid-prefix",
					message: expect.stringContaining("prefix"),
				}),
			);
		});

		it("no error for prefix on date parameter", () => {
			const result = diagnose("/fhir/Patient?birthdate=ge2000-01-01");
			expect(result.filter((d) => d.code === "invalid-prefix")).toHaveLength(0);
		});

		it("reports prefix on token parameter", () => {
			const result = diagnose("/fhir/Patient?active=ne1");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "error",
					code: "invalid-prefix",
				}),
			);
		});

		it("no error for prefix on quantity parameter", () => {
			const result = diagnose("/fhir/Observation?value-quantity=gt100");
			expect(result.filter((d) => d.code === "invalid-prefix")).toHaveLength(0);
		});
	});

	describe("special param validation", () => {
		it("reports invalid _count value", () => {
			const result = diagnose("/fhir/Patient?_count=abc");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "error",
					code: "invalid-value",
					message: expect.stringContaining("_count"),
				}),
			);
		});

		it("reports _count=0", () => {
			const result = diagnose("/fhir/Patient?_count=0");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "error",
					code: "invalid-value",
					message: expect.stringContaining("_count"),
				}),
			);
		});

		it("no error for valid _count", () => {
			const result = diagnose("/fhir/Patient?_count=10");
			expect(result.filter((d) => d.message.includes("_count"))).toHaveLength(0);
		});

		it("reports invalid _summary value", () => {
			const result = diagnose("/fhir/Patient?_summary=invalid");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "error",
					code: "invalid-value",
					message: expect.stringContaining("_summary"),
				}),
			);
		});

		it("no error for valid _summary", () => {
			const result = diagnose("/fhir/Patient?_summary=count");
			expect(result.filter((d) => d.message.includes("_summary"))).toHaveLength(0);
		});

		it("reports invalid _total value", () => {
			const result = diagnose("/fhir/Patient?_total=all");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "error",
					code: "invalid-value",
					message: expect.stringContaining("_total"),
				}),
			);
		});

		it("reports invalid _offset value", () => {
			const result = diagnose("/fhir/Patient?_offset=abc");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "error",
					code: "invalid-value",
					message: expect.stringContaining("_offset"),
				}),
			);
		});
	});

	describe("_sort validation", () => {
		it("warns about unknown sort parameter", () => {
			const result = diagnose("/fhir/Patient?_sort=unknown");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "warning",
					code: "invalid-value",
					message: expect.stringContaining("sort"),
				}),
			);
		});

		it("no warning for valid sort param", () => {
			const result = diagnose("/fhir/Patient?_sort=name");
			expect(result.filter((d) => d.message.includes("sort"))).toHaveLength(0);
		});

		it("handles descending sort prefix", () => {
			const result = diagnose("/fhir/Patient?_sort=-name");
			expect(result.filter((d) => d.message.includes("sort"))).toHaveLength(0);
		});
	});

	describe("_include validation", () => {
		it("warns about unknown _include", () => {
			const result = diagnose("/fhir/Patient?_include=Patient:fake:Target");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "warning",
					code: "invalid-value",
				}),
			);
		});

		it("no warning for wildcard _include", () => {
			const result = diagnose("/fhir/Patient?_include=*");
			expect(result.filter((d) => d.message.includes("_include"))).toHaveLength(0);
		});
	});

	describe("duplicate params", () => {
		it("warns about duplicate non-repeatable params", () => {
			const result = diagnose("/fhir/Patient?name=a&name=b");
			expect(result).toContainEqual(
				expect.objectContaining({
					severity: "warning",
					code: "duplicate-param",
				}),
			);
		});

		it("no warning for repeatable params like _include", () => {
			const result = diagnose("/fhir/Patient?_include=Patient:general-practitioner:Practitioner&_include=Patient:general-practitioner:Organization");
			expect(result.filter((d) => d.code === "duplicate-param")).toHaveLength(0);
		});
	});

	describe("graceful degradation", () => {
		it("no false positives without search params metadata", () => {
			const result = diagnose("/fhir/Patient?name=John&xyz=1", {
				searchParamsByResource: {},
			});
			// Without metadata for Patient, we can't validate params
			expect(result.filter((d) => d.code === "unknown-param")).toHaveLength(0);
		});

		it("no false positives without capabilities", () => {
			const result = diagnose("/fhir/Patient?_sort=xyz&_include=fake", {
				capabilities: undefined,
			});
			// Without capabilities, can't validate _sort or _include values
			expect(result.filter((d) => d.message.includes("sort"))).toHaveLength(0);
			expect(result.filter((d) => d.message.includes("_include"))).toHaveLength(0);
		});

		it("handles api-endpoint paths without errors", () => {
			const result = diagnose("/api/__introspect/rest-console");
			expect(result).toHaveLength(0);
		});

		it("handles root path without errors", () => {
			const result = diagnose("/fhir/");
			expect(result).toHaveLength(0);
		});
	});
});

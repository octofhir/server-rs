import { describe, it, expect } from "vitest";
import { getCursorContext } from "./cursor-context";

const RESOURCE_TYPES = [
	"Patient",
	"Observation",
	"Condition",
	"Practitioner",
	"Organization",
];

describe("getCursorContext", () => {
	it("returns root at start of input", () => {
		const ctx = getCursorContext("/", 1, RESOURCE_TYPES);
		expect(ctx.type).toBe("root");
	});

	it("returns root for empty string", () => {
		const ctx = getCursorContext("", 0, RESOURCE_TYPES);
		expect(ctx.type).toBe("root");
	});

	it("returns resource-type when typing after /fhir/", () => {
		const ctx = getCursorContext("/fhir/Pat", 9, RESOURCE_TYPES);
		expect(ctx.type).toBe("resource-type");
		expect(ctx.fragment).toBe("Pat");
	});

	it("returns next-after-resource for complete resource type", () => {
		const ctx = getCursorContext("/fhir/Patient", 14, RESOURCE_TYPES);
		expect(ctx.type).toBe("next-after-resource");
		expect(ctx.resourceType).toBe("Patient");
	});

	it("returns resource-id after trailing slash", () => {
		const ctx = getCursorContext("/fhir/Patient/", 15, RESOURCE_TYPES);
		expect(ctx.type).toBe("resource-id");
	});

	it("returns next-after-id for resource with id", () => {
		const ctx = getCursorContext("/fhir/Patient/123", 18, RESOURCE_TYPES);
		expect(ctx.type).toBe("next-after-id");
		expect(ctx.resourceType).toBe("Patient");
		expect(ctx.resourceId).toBe("123");
	});

	it("returns type-operation for $", () => {
		const ctx = getCursorContext("/fhir/Patient/$val", 19, RESOURCE_TYPES);
		expect(ctx.type).toBe("type-operation");
		expect(ctx.fragment).toBe("$val");
	});

	it("returns query-param after ?", () => {
		const ctx = getCursorContext("/fhir/Patient?nam", 18, RESOURCE_TYPES);
		expect(ctx.type).toBe("query-param");
		expect(ctx.resourceType).toBe("Patient");
		expect(ctx.fragment).toBe("nam");
	});

	it("returns query-param after &", () => {
		const ctx = getCursorContext(
			"/fhir/Patient?name=John&birth",
			29,
			RESOURCE_TYPES,
		);
		expect(ctx.type).toBe("query-param");
		expect(ctx.fragment).toBe("birth");
	});

	it("returns query-modifier for colon without equals", () => {
		const ctx = getCursorContext(
			"/fhir/Patient?name:ex",
			21,
			RESOURCE_TYPES,
		);
		expect(ctx.type).toBe("query-modifier");
		expect(ctx.paramName).toBe("name");
		expect(ctx.fragment).toBe("ex");
	});

	it("returns query-value after equals", () => {
		const ctx = getCursorContext(
			"/fhir/Patient?name=Jo",
			21,
			RESOURCE_TYPES,
		);
		expect(ctx.type).toBe("query-value");
		expect(ctx.paramName).toBe("name");
		expect(ctx.fragment).toBe("Jo");
	});

	it("returns api-endpoint for /api paths", () => {
		const ctx = getCursorContext("/api/__intro", 12, RESOURCE_TYPES);
		expect(ctx.type).toBe("api-endpoint");
	});

	it("has correct span for fragment", () => {
		const ctx = getCursorContext("/fhir/Pat", 9, RESOURCE_TYPES);
		expect(ctx.span.start).toBe(6);
		expect(ctx.span.end).toBe(9);
	});

	it("returns resource-type for empty after /fhir/", () => {
		const ctx = getCursorContext("/fhir/", 6, RESOURCE_TYPES);
		expect(ctx.type).toBe("resource-type");
		expect(ctx.fragment).toBe("");
	});

	it("returns instance-operation for /fhir/Patient/123/$", () => {
		const ctx = getCursorContext(
			"/fhir/Patient/123/$every",
			24,
			RESOURCE_TYPES,
		);
		expect(ctx.type).toBe("instance-operation");
		expect(ctx.fragment).toBe("$every");
	});
});

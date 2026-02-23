import { describe, it, expect } from "vitest";
import { parseQueryAst } from "./parser";
import { serializeAst } from "./serializer";

describe("parseQueryAst", () => {
	it("parses resource-type path", () => {
		const ast = parseQueryAst("/fhir/Patient");
		expect(ast.path.kind).toBe("resource-type");
		if (ast.path.kind === "resource-type") {
			expect(ast.path.resourceType).toBe("Patient");
		}
		expect(ast.params).toHaveLength(0);
	});

	it("parses resource-instance path", () => {
		const ast = parseQueryAst("/fhir/Patient/123");
		expect(ast.path.kind).toBe("resource-instance");
		if (ast.path.kind === "resource-instance") {
			expect(ast.path.resourceType).toBe("Patient");
			expect(ast.path.id).toBe("123");
		}
	});

	it("parses type-operation path", () => {
		const ast = parseQueryAst("/fhir/Patient/$validate");
		expect(ast.path.kind).toBe("type-operation");
		if (ast.path.kind === "type-operation") {
			expect(ast.path.resourceType).toBe("Patient");
			expect(ast.path.operation).toBe("$validate");
		}
	});

	it("parses instance-operation path", () => {
		const ast = parseQueryAst("/fhir/Patient/123/$everything");
		expect(ast.path.kind).toBe("instance-operation");
		if (ast.path.kind === "instance-operation") {
			expect(ast.path.resourceType).toBe("Patient");
			expect(ast.path.id).toBe("123");
			expect(ast.path.operation).toBe("$everything");
		}
	});

	it("parses system-operation path", () => {
		const ast = parseQueryAst("/fhir/$export");
		expect(ast.path.kind).toBe("system-operation");
		if (ast.path.kind === "system-operation") {
			expect(ast.path.operation).toBe("$export");
		}
	});

	it("parses query params with values", () => {
		const ast = parseQueryAst(
			"/fhir/Patient?name=John&birthdate=ge2000-01-01",
		);
		expect(ast.params).toHaveLength(2);
		expect(ast.params[0].name).toBe("name");
		expect(ast.params[0].values[0].raw).toBe("John");
		expect(ast.params[0].values[0].prefix).toBeUndefined();
		expect(ast.params[1].name).toBe("birthdate");
		expect(ast.params[1].values[0].raw).toBe("ge2000-01-01");
		expect(ast.params[1].values[0].prefix).toBe("ge");
	});

	it("parses modifier", () => {
		const ast = parseQueryAst("/fhir/Patient?name:exact=John");
		expect(ast.params).toHaveLength(1);
		expect(ast.params[0].name).toBe("name");
		expect(ast.params[0].modifier).toBe("exact");
		expect(ast.params[0].values[0].raw).toBe("John");
	});

	it("parses api-endpoint path", () => {
		const ast = parseQueryAst("/api/__introspect/rest-console");
		expect(ast.path.kind).toBe("api-endpoint");
		if (ast.path.kind === "api-endpoint") {
			expect(ast.path.path).toBe("/api/__introspect/rest-console");
		}
	});

	it("parses root path", () => {
		const ast = parseQueryAst("/fhir/");
		expect(ast.path.kind).toBe("root");
	});

	it("marks special params", () => {
		const ast = parseQueryAst("/fhir/Patient?_count=10&name=John");
		expect(ast.params[0].isSpecial).toBe(true);
		expect(ast.params[1].isSpecial).toBe(false);
	});

	it("parses OR values (comma-separated)", () => {
		const ast = parseQueryAst("/fhir/Patient?status=active,inactive");
		expect(ast.params[0].values).toHaveLength(2);
		expect(ast.params[0].values[0].raw).toBe("active");
		expect(ast.params[0].values[1].raw).toBe("inactive");
	});

	it("has correct spans for path", () => {
		const raw = "/fhir/Patient";
		const ast = parseQueryAst(raw);
		expect(ast.path.span.start).toBe(0);
		expect(ast.path.span.end).toBe(raw.length);
	});

	it("has correct spans for params", () => {
		const raw = "/fhir/Patient?name=John&_count=10";
		const ast = parseQueryAst(raw);
		// "name=John" starts after "?" at index 14
		expect(ast.params[0].span.start).toBe(14);
		// "&_count=10" — _count starts after "&" at index 23+1=24
		expect(ast.params[1].name).toBe("_count");
	});

	it("parses empty string as root", () => {
		const ast = parseQueryAst("");
		expect(ast.path.kind).toBe("root");
	});

	it("parses resource type with trailing slash", () => {
		const ast = parseQueryAst("/fhir/Patient/");
		// Trailing slash is stripped, still a resource-type
		expect(ast.path.kind).toBe("resource-type");
		if (ast.path.kind === "resource-type") {
			expect(ast.path.resourceType).toBe("Patient");
		}
	});
});

describe("serializeAst round-trip", () => {
	const testCases = [
		"/fhir/Patient",
		"/fhir/Patient/123",
		"/fhir/Patient/$validate",
		"/fhir/Patient/123/$everything",
		"/fhir/$export",
		"/fhir/Patient?name=John",
		"/fhir/Patient?name=John&birthdate=ge2000-01-01",
		"/fhir/Patient?name:exact=John",
		"/fhir/Observation?_count=10&_sort=-date",
		"/api/__introspect/rest-console",
	];

	for (const input of testCases) {
		it(`round-trips: ${input}`, () => {
			const ast = parseQueryAst(input);
			const serialized = serializeAst(ast);
			expect(serialized).toBe(input);
		});
	}
});

describe("serializeAst", () => {
	it("serializes root as basePath", () => {
		const ast = parseQueryAst("/fhir/");
		const result = serializeAst(ast);
		expect(result).toBe("/fhir");
	});
});

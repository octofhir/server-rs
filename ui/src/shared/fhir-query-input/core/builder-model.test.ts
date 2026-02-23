import { describe, it, expect } from "vitest";
import { astToBuilderState, builderStateToAst, builderStateToRaw } from "./builder-model";
import { parseQueryAst } from "./parser";
import { serializeAst } from "./serializer";

describe("builder-model", () => {
	describe("astToBuilderState", () => {
		it("extracts resource type from simple search", () => {
			const ast = parseQueryAst("/fhir/Patient");
			const state = astToBuilderState(ast);
			expect(state.resourceType).toBe("Patient");
			expect(state.resourceId).toBeUndefined();
			expect(state.operation).toBeUndefined();
		});

		it("extracts resource instance", () => {
			const ast = parseQueryAst("/fhir/Patient/123");
			const state = astToBuilderState(ast);
			expect(state.resourceType).toBe("Patient");
			expect(state.resourceId).toBe("123");
		});

		it("extracts type operation", () => {
			const ast = parseQueryAst("/fhir/Patient/$validate");
			const state = astToBuilderState(ast);
			expect(state.resourceType).toBe("Patient");
			expect(state.operation).toBe("$validate");
		});

		it("extracts instance operation", () => {
			const ast = parseQueryAst("/fhir/Patient/123/$everything");
			const state = astToBuilderState(ast);
			expect(state.resourceType).toBe("Patient");
			expect(state.resourceId).toBe("123");
			expect(state.operation).toBe("$everything");
		});

		it("extracts system operation", () => {
			const ast = parseQueryAst("/fhir/$export");
			const state = astToBuilderState(ast);
			expect(state.resourceType).toBeUndefined();
			expect(state.operation).toBe("$export");
		});

		it("extracts query params", () => {
			const ast = parseQueryAst("/fhir/Patient?name=John&birthdate=ge2000-01-01");
			const state = astToBuilderState(ast);
			expect(state.params).toHaveLength(2);
			expect(state.params[0].code).toBe("name");
			expect(state.params[0].value).toBe("John");
			expect(state.params[0].isSpecial).toBe(false);
			expect(state.params[1].code).toBe("birthdate");
			expect(state.params[1].value).toBe("ge2000-01-01");
		});

		it("extracts modifier", () => {
			const ast = parseQueryAst("/fhir/Patient?name:exact=John");
			const state = astToBuilderState(ast);
			expect(state.params[0].modifier).toBe("exact");
		});

		it("identifies special params", () => {
			const ast = parseQueryAst("/fhir/Patient?_count=10&_sort=-name");
			const state = astToBuilderState(ast);
			expect(state.params[0].isSpecial).toBe(true);
			expect(state.params[1].isSpecial).toBe(true);
		});
	});

	describe("builderStateToAst", () => {
		it("creates resource search AST", () => {
			const ast = builderStateToAst({
				resourceType: "Patient",
				params: [],
			});
			expect(ast.path.kind).toBe("resource-type");
			if (ast.path.kind === "resource-type") {
				expect(ast.path.resourceType).toBe("Patient");
			}
		});

		it("creates resource instance AST", () => {
			const ast = builderStateToAst({
				resourceType: "Patient",
				resourceId: "123",
				params: [],
			});
			expect(ast.path.kind).toBe("resource-instance");
		});

		it("creates AST with params", () => {
			const ast = builderStateToAst({
				resourceType: "Patient",
				params: [
					{ id: "1", code: "name", value: "John", isSpecial: false },
					{ id: "2", code: "_count", value: "10", isSpecial: true },
				],
			});
			expect(ast.params).toHaveLength(2);
			expect(ast.params[0].name).toBe("name");
			expect(ast.params[1].name).toBe("_count");
		});
	});

	describe("round-trip", () => {
		const cases = [
			"/fhir/Patient",
			"/fhir/Patient/123",
			"/fhir/Patient/$validate",
			"/fhir/Patient/123/$everything",
			"/fhir/$export",
			"/fhir/Patient?name=John",
			"/fhir/Patient?name=John&birthdate=ge2000-01-01",
			"/fhir/Patient?name:exact=John",
			"/fhir/Patient?_count=10&_sort=-name",
		];

		for (const input of cases) {
			it(`round-trips: ${input}`, () => {
				const ast = parseQueryAst(input);
				const state = astToBuilderState(ast);
				const output = builderStateToRaw(state);
				expect(output).toBe(input);
			});
		}
	});
});

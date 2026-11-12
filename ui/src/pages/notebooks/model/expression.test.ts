import { describe, expect, it } from "vitest";
import { compileExpression } from "./expression";

describe("notebook expression isolation", () => {
  it("supports formulas, comparisons, strings and data variables", () => {
    expect(compileExpression("Math.round(price * ${factor})", { factor: 1.5 })({ price: 3 })).toBe(
      5
    );
    expect(
      compileExpression(
        'name.toLowerCase().includes("ann") && age >= 18',
        {}
      )({ name: "ANNA", age: 22 })
    ).toBe(true);
    expect(compileExpression("active ? count + 1 : 0", {})({ active: true, count: 4 })).toBe(5);
    expect(compileExpression('"${value}"', { value: 'x" + fetch("/admin") + "' })({})).toBe(
      'x" + fetch("/admin") + "'
    );
    expect(compileExpression("${value}", { value: "globalThis" })({})).toBe("globalThis");
  });

  it("rejects access to globals, constructors, assignments and arbitrary calls", () => {
    for (const expression of [
      "globalThis",
      "window",
      "sessionStorage",
      "fetch('/admin')",
      "({}).constructor.constructor('return globalThis')()",
      "name.constructor('return globalThis')()",
      "value['__proto__']",
      "(globalThis.marker = 1)",
      "(() => 1)()",
      "Math['constructor']('return 1')()",
      "1; fetch('/admin')",
    ]) {
      expect(() => compileExpression(expression, {})({ name: "test", value: {} })).toThrow();
    }
  });
});

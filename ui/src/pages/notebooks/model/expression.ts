// Interpret a bounded expression AST. Never turn notebook expressions or values
// into JavaScript source executed in the authenticated application's origin.
import { parseExpressionAt, type Node } from "acorn";
import type { Scope } from "./notebook";

type Expr = Node & {
  name?: string;
  value?: unknown;
  operator?: string;
  left?: Expr;
  right?: Expr;
  argument?: Expr;
  test?: Expr;
  consequent?: Expr;
  alternate?: Expr;
  object?: Expr;
  property?: Expr;
  computed?: boolean;
  callee?: Expr;
  arguments?: Expr[];
  expression?: Expr;
};

const forbidden = new Set(["__proto__", "constructor", "prototype"]);
const math: Record<string, (...args: number[]) => number> = {
  abs: Math.abs,
  ceil: Math.ceil,
  floor: Math.floor,
  round: Math.round,
  min: Math.min,
  max: Math.max,
  sqrt: Math.sqrt,
  pow: Math.pow,
  trunc: Math.trunc,
};

function own(object: unknown, key: string): unknown {
  if (forbidden.has(key)) throw new Error("Property is not allowed in expressions");
  if (typeof object === "string" && key === "length") return object.length;
  return object != null && typeof object === "object" && Object.hasOwn(object, key)
    ? (object as Record<string, unknown>)[key]
    : undefined;
}

function scopeValue(scope: Scope, path: string): unknown {
  return path
    .replace(/\[(\d+)\]/g, ".$1")
    .split(".")
    .reduce<unknown>(own, scope);
}

export function compileExpression(
  source: string,
  scope: Scope
): (row: Record<string, unknown>) => unknown {
  if (source.length > 8192) throw new Error("Expression is too long");
  let prefix = "__notebook_value_";
  while (source.includes(prefix)) prefix += "_";
  const values = new Map<string, unknown>();
  const text = source.replace(/\$\{([a-zA-Z_][\w.[\]]*)\}/g, (_match, path: string) => {
    const name = `${prefix}${values.size}`;
    values.set(name, scopeValue(scope, path));
    return name;
  });
  const ast = parseExpressionAt(text, 0, { ecmaVersion: 2022, preserveParens: true }) as Expr;
  if (text.slice(ast.end).trim()) throw new Error("Only one expression is allowed");

  return (row) => {
    let budget = 512;
    function evaluate(node: Expr, depth = 0): unknown {
      if (--budget < 0 || depth > 64) throw new Error("Expression is too complex");
      const next = (child: Expr | undefined) => {
        if (!child) throw new Error("Invalid expression");
        return evaluate(child, depth + 1);
      };
      switch (node.type) {
        case "ParenthesizedExpression":
          return next(node.expression);
        case "Literal": {
          if (typeof node.value !== "string") return node.value;
          let literal = node.value;
          for (const [name, value] of values) {
            literal = literal.replaceAll(
              name,
              typeof value === "object" ? JSON.stringify(value) : String(value)
            );
          }
          return literal;
        }
        case "Identifier":
          if (values.has(node.name!)) return values.get(node.name!);
          if (node.name === "undefined") return undefined;
          if (forbidden.has(node.name!)) throw new Error("Identifier is not allowed");
          if (!Object.hasOwn(row, node.name!)) throw new Error(`Unknown column: ${node.name}`);
          return row[node.name!];
        case "MemberExpression": {
          const key = node.computed ? String(next(node.property)) : node.property!.name!;
          return own(next(node.object), key);
        }
        case "ConditionalExpression":
          return next(node.test) ? next(node.consequent) : next(node.alternate);
        case "LogicalExpression": {
          const left = next(node.left);
          if (node.operator === "&&") return left && next(node.right);
          if (node.operator === "||") return left || next(node.right);
          if (node.operator === "??") return left ?? next(node.right);
          throw new Error("Unsupported operator");
        }
        case "UnaryExpression": {
          const value = next(node.argument);
          switch (node.operator) {
            case "!":
              return !value;
            case "+":
              return Number(value);
            case "-":
              return -Number(value);
            case "typeof":
              return typeof value;
            default:
              throw new Error("Unsupported unary operator");
          }
        }
        case "BinaryExpression": {
          const left = next(node.left);
          const right = next(node.right);
          switch (node.operator) {
            case "+":
              return typeof left === "string" || typeof right === "string"
                ? String(left) + String(right)
                : Number(left) + Number(right);
            case "-":
              return Number(left) - Number(right);
            case "*":
              return Number(left) * Number(right);
            case "/":
              return Number(left) / Number(right);
            case "%":
              return Number(left) % Number(right);
            case "**":
              return Number(left) ** Number(right);
            case "===":
            case "==":
              return left === right;
            case "!==":
            case "!=":
              return left !== right;
            case ">":
              return typeof left === "string" && typeof right === "string"
                ? left > right
                : Number(left) > Number(right);
            case ">=":
              return typeof left === "string" && typeof right === "string"
                ? left >= right
                : Number(left) >= Number(right);
            case "<":
              return typeof left === "string" && typeof right === "string"
                ? left < right
                : Number(left) < Number(right);
            case "<=":
              return typeof left === "string" && typeof right === "string"
                ? left <= right
                : Number(left) <= Number(right);
            default:
              throw new Error("Unsupported binary operator");
          }
        }
        case "CallExpression": {
          const callee = node.callee!;
          const args = (node.arguments ?? []).map((argument) => next(argument));
          if (callee.type === "Identifier") {
            if (callee.name === "Number") return Number(args[0]);
            if (callee.name === "String") return String(args[0]);
            if (callee.name === "Boolean") return Boolean(args[0]);
          }
          if (callee.type === "MemberExpression" && !callee.computed) {
            const method = callee.property!.name!;
            if (
              callee.object!.type === "Identifier" &&
              callee.object!.name === "Math" &&
              Object.hasOwn(math, method)
            ) {
              return math[method](...args.map(Number));
            }
            const object = next(callee.object);
            if (typeof object === "string") {
              switch (method) {
                case "includes":
                  return object.includes(String(args[0]));
                case "startsWith":
                  return object.startsWith(String(args[0]));
                case "endsWith":
                  return object.endsWith(String(args[0]));
                case "toLowerCase":
                  return object.toLowerCase();
                case "toUpperCase":
                  return object.toUpperCase();
                case "trim":
                  return object.trim();
              }
            }
          }
          throw new Error("Function is not allowed in expressions");
        }
        default:
          throw new Error(`Unsupported expression: ${node.type}`);
      }
    }
    return evaluate(ast);
  };
}

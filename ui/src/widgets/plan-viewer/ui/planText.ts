interface PgPlanNode {
  "Node Type"?: string;
  "Relation Name"?: string;
  "Index Name"?: string;
  Alias?: string;
  "Join Type"?: string;
  "Startup Cost"?: number;
  "Total Cost"?: number;
  "Plan Rows"?: number;
  "Actual Rows"?: number;
  "Actual Total Time"?: number;
  "Actual Loops"?: number;
  "Rows Removed by Filter"?: number;
  "Index Cond"?: string;
  Filter?: string;
  "Hash Cond"?: string;
  Plans?: PgPlanNode[];
  [key: string]: unknown;
}

interface PgPlanRoot {
  Plan?: PgPlanNode;
  "Planning Time"?: number;
  "Execution Time"?: number;
}

/** Coerce raw EXPLAIN (FORMAT JSON) output into the plan-root array. */
export function normalizePlan(plan: unknown): PgPlanRoot[] | null {
  let value = plan;
  if (typeof value === "string") {
    try {
      value = JSON.parse(value);
    } catch {
      return null;
    }
  }
  if (Array.isArray(value) && value.length > 0 && typeof value[0] === "object") {
    return value as PgPlanRoot[];
  }
  if (value && typeof value === "object" && "Plan" in value) {
    return [value as PgPlanRoot];
  }
  return null;
}

/** Render a parsed JSON plan as an indented, human-readable text tree. */
export function planToText(plan: unknown): string {
  const roots = normalizePlan(plan);
  if (!roots) return "";
  const root = roots[0];
  const lines: string[] = [];

  const walk = (node: PgPlanNode | undefined, depth: number) => {
    if (!node) return;
    const indent = depth === 0 ? "" : `${"  ".repeat(depth - 1)}->  `;
    const parts: string[] = [node["Node Type"] ?? "Node"];
    if (node["Join Type"]) parts.push(`(${node["Join Type"]})`);
    if (node["Index Name"]) parts.push(`using ${node["Index Name"]}`);
    if (node["Relation Name"]) {
      parts.push(`on ${node["Relation Name"]}`);
      if (node.Alias && node.Alias !== node["Relation Name"]) parts.push(node.Alias);
    }

    const cost =
      node["Total Cost"] != null
        ? `  (cost=${node["Startup Cost"]?.toFixed(2) ?? "0.00"}..${node["Total Cost"].toFixed(2)} rows=${node["Plan Rows"] ?? "?"})`
        : "";
    const actual =
      node["Actual Total Time"] != null
        ? `  (actual time=${node["Actual Total Time"].toFixed(3)} rows=${node["Actual Rows"] ?? "?"} loops=${node["Actual Loops"] ?? 1})`
        : "";

    lines.push(`${indent}${parts.join(" ")}${cost}${actual}`);

    const detailIndent = `${"  ".repeat(depth)}      `;
    if (node["Index Cond"]) lines.push(`${detailIndent}Index Cond: ${node["Index Cond"]}`);
    if (node["Hash Cond"]) lines.push(`${detailIndent}Hash Cond: ${node["Hash Cond"]}`);
    if (node.Filter) lines.push(`${detailIndent}Filter: ${node.Filter}`);
    if (node["Rows Removed by Filter"]) {
      lines.push(`${detailIndent}Rows Removed by Filter: ${node["Rows Removed by Filter"]}`);
    }

    for (const child of node.Plans ?? []) walk(child, depth + 1);
  };

  walk(root?.Plan, 0);

  if (root?.["Planning Time"] != null) {
    lines.push(`Planning Time: ${root["Planning Time"].toFixed(3)} ms`);
  }
  if (root?.["Execution Time"] != null) {
    lines.push(`Execution Time: ${root["Execution Time"].toFixed(3)} ms`);
  }

  return lines.join("\n");
}

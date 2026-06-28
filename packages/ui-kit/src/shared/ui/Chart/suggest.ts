import { discoverFields } from "./discoverFields";
import { isNumericType } from "./inferColumn";
import type { ChartSpec, DerivedField, FieldDef, TabularData } from "./types";

function toDerived(fields: FieldDef[]): DerivedField[] {
    return fields.map((f) => ({ name: f.name, path: f.path, array: f.array, type: f.type }));
}

/** Identifier-ish numeric/text fields make terrible measures (e.g. `txid`). */
function isIdLike(f: FieldDef): boolean {
    if (f.type === "uuid") return true;
    const leaf = (f.name.toLowerCase().split(/[.[]/).pop() ?? "").replace(/[\]]/g, "");
    return leaf === "id" || leaf === "txid" || leaf === "guid" || leaf.endsWith("_id");
}

const COUNT_MEASURE = { column: "*", agg: "count" as const };

/**
 * Suggest a starting chart spec for arbitrary tabular data — JSON-aware. Walks
 * the data into a flat field catalog (so nested FHIR fields are considered),
 * picks a dimension + numeric measures, and records the referenced fields in
 * `derive` so the extraction survives serialization.
 */
export function suggestChartSpec(data: TabularData): ChartSpec {
    const fields = discoverFields(data);
    const scalar = fields.filter((f) => !f.array);
    // Real measures only — drop id/uuid columns that sum to nonsense.
    const numeric = scalar.filter((f) => isNumericType(f.type) && !isIdLike(f));
    const dateField = scalar.find((f) => f.type === "date");
    const textField = scalar.find((f) => f.type === "text" || f.type === "bool");

    // Two real numeric measures, no clear dimension → scatter.
    if (numeric.length >= 2 && !dateField && !textField) {
        return {
            type: "scatter",
            x: numeric[0].name,
            series: [{ column: numeric[1].name }],
            legend: false,
            derive: toDerived([numeric[0], numeric[1]]),
        };
    }

    const xField = dateField ?? textField ?? scalar[0];
    const measures = numeric.filter((f) => f.name !== xField?.name).slice(0, 4);
    const used = [xField, ...measures].filter((f): f is FieldDef => Boolean(f));
    const onDate = Boolean(dateField && xField?.name === dateField.name);

    // Prefer counting rows when there's no meaningful numeric measure — the
    // common "count by category / per year" case for FHIR resources.
    const series = measures.length
        ? measures.map((f) => ({ column: f.name, agg: "sum" as const }))
        : [COUNT_MEASURE];

    return {
        type: dateField ? "line" : "bar",
        x: xField?.name,
        series,
        legend: measures.length > 1,
        ...(onDate ? { xBucket: "year" as const } : {}),
        derive: toDerived(used),
    };
}

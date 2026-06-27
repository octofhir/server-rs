import { useMutation } from "@tanstack/react-query";
import { fhirClient } from "@/shared/api/fhirClient";

export interface ExplainPredicate {
  param_code: string;
  search_type: string;
  strategy: string;
  expected_index: string | null;
  index_backed: boolean;
  sql_shape: string;
}

export interface ExplainResult {
  resource_type: string;
  parsed_ir: { resource_type: string; predicates: ExplainPredicate[] } | null;
  sql: string;
  params: string[];
  unknown_params: { name: string; modifier: string | null }[];
  analyzed: boolean;
  explain_plan: unknown;
}

/** Split a console path like "/fhir/Patient?name=x" into resource type + query string. */
export function parseExplainTarget(rawPath: string): { resourceType: string; query: string } {
  let path = rawPath.trim();
  const qIndex = path.indexOf("?");
  const query = qIndex >= 0 ? path.slice(qIndex + 1) : "";
  path = qIndex >= 0 ? path.slice(0, qIndex) : path;
  // Strip leading slash and an optional /fhir prefix.
  path = path.replace(/^\/+/, "").replace(/^fhir\/+/, "");
  const resourceType = path.split("/")[0] ?? "";
  return { resourceType, query };
}

export function useExplainQuery() {
  return useMutation({
    mutationFn: async (input: { resourceType: string; query: string; analyze?: boolean }) => {
      const res = await fhirClient.customRequest({
        method: "POST",
        url: "/api/console/explain",
        data: {
          resource_type: input.resourceType,
          query: input.query,
          analyze: input.analyze ?? false,
        },
      });
      return res.data as ExplainResult;
    },
  });
}

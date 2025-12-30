import { useQuery } from "@tanstack/react-query";

/**
 * Fetch available FHIR resource types from the server.
 *
 * Uses the lightweight /api/resource-types endpoint which returns
 * a simple array of resource type strings.
 */
export function useResourceTypes() {
  return useQuery({
    queryKey: ["resourceTypes"],
    queryFn: async (): Promise<string[]> => {
      const res = await fetch("/api/resource-types");
      if (!res.ok) {
        throw new Error("Failed to fetch resource types");
      }
      return res.json();
    },
    staleTime: Infinity, // Resource types don't change during a session
  });
}

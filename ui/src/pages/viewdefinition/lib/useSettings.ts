import { useQuery } from "@tanstack/react-query";

export interface ServerFeatures {
  sqlOnFhir: boolean;
  graphql: boolean;
  bulkExport: boolean;
  dbConsole: boolean;
  auth: boolean;
}

export interface ServerSettings {
  fhirVersion: string;
  features: ServerFeatures;
}

async function fetchSettings(): Promise<ServerSettings> {
  const res = await fetch("/api/settings");
  if (!res.ok) {
    throw new Error("Failed to fetch settings");
  }
  return res.json();
}

export function useSettings() {
  return useQuery({
    queryKey: ["settings"],
    queryFn: fetchSettings,
    staleTime: 5 * 60 * 1000, // 5 minutes
  });
}

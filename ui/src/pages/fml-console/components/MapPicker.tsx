/**
 * Opens a StructureMap stored on the server in the editor: lists the maps, prints the chosen
 * one back to FML (`/api/fml/print`).
 */

import { Select } from "@octofhir/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { fhirClient } from "@/shared/api/fhirClient";
import { isRecord } from "@/shared/api/guards";

interface StoredMap {
  id: string;
  label: string;
}

async function storedMaps(): Promise<StoredMap[]> {
  const response = await fhirClient.customRequest({
    method: "GET",
    url: "/fhir/StructureMap?_count=200&_elements=id,url,name,title&_sort=name",
  });
  const bundle = response.data;
  if (!isRecord(bundle) || !Array.isArray(bundle.entry)) return [];
  return bundle.entry
    .map((entry) => (isRecord(entry) && isRecord(entry.resource) ? entry.resource : undefined))
    .filter((resource): resource is Record<string, unknown> => !!resource && typeof resource.id === "string")
    .map((resource) => ({
      id: resource.id as string,
      label: String(resource.title ?? resource.name ?? resource.url ?? resource.id),
    }));
}

export async function openStoredMap(id: string): Promise<string> {
  const map = await fhirClient.customRequest({ method: "GET", url: `/fhir/StructureMap/${id}` });
  const printed = await fhirClient.customRequest({
    method: "POST",
    url: "/api/fml/print",
    data: map.data,
    headers: { "Content-Type": "application/json" },
  });
  if (isRecord(printed.data) && typeof printed.data.source === "string") return printed.data.source;
  throw new Error("The server did not print the map");
}

export interface MapPickerProps {
  onOpen: (id: string) => void;
}

export function MapPicker({ onOpen }: MapPickerProps) {
  const maps = useQuery({ queryKey: ["fml-console", "maps"], queryFn: storedMaps, staleTime: 30_000 });
  return (
    <Select
      size="sm"
      w={220}
      placeholder={maps.isLoading ? "Loading maps…" : "Open a stored map"}
      data={(maps.data ?? []).map((map) => ({ value: map.id, label: map.label }))}
      value={null}
      searchable
      onChange={(id) => id && onOpen(id)}
    />
  );
}

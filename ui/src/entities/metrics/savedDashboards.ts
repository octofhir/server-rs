import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { fhirClient } from "@/shared/api/fhirClient";
import type { FhirResource } from "@/shared/api/types";
import type { Dashboard } from "./dashboard";

/**
 * Saved dashboards are `Dashboard` resources (octofhir-ui IG) behind the generic FHIR CRUD, so
 * they get history, access policies and `PUT /fhir/Dashboard/{id}` for provisioning by script.
 * The IG models only header fields; panels are open JSON, hence `X-Skip-Validation` on writes.
 */

type DashboardResource = Dashboard & FhirResource & { resourceType: "Dashboard" };

const SKIP_VALIDATION = { "X-Skip-Validation": "true" };

const savedKeys = {
  all: ["metrics", "saved-dashboards"] as const,
  one: (id: string) => [...savedKeys.all, id] as const,
};

function toDashboard(resource: DashboardResource): Dashboard {
  const { resourceType: _type, meta: _meta, ...dashboard } = resource;
  return dashboard as Dashboard;
}

async function listDashboards(): Promise<Dashboard[]> {
  const bundle = await fhirClient.search<DashboardResource>("Dashboard", { _count: 200, _sort: "title" });
  return (bundle.entry ?? [])
    .map((entry) => entry.resource)
    .filter((resource): resource is DashboardResource => resource != null)
    .map(toDashboard);
}

async function saveDashboard(dashboard: Dashboard): Promise<Dashboard> {
  const body = { ...dashboard, resourceType: "Dashboard" };
  const response = dashboard.id
    ? await fhirClient.customRequest({
        method: "PUT",
        url: `/fhir/Dashboard/${dashboard.id}`,
        data: body,
        headers: SKIP_VALIDATION,
      })
    : await fhirClient.customRequest({
        method: "POST",
        url: "/fhir/Dashboard",
        data: (({ id: _id, ...rest }) => rest)(body),
        headers: SKIP_VALIDATION,
      });
  return toDashboard(response.data as DashboardResource);
}

export function useSavedDashboards(enabled = true) {
  return useQuery({ queryKey: savedKeys.all, queryFn: listDashboards, enabled, staleTime: 30_000 });
}

export function useSaveDashboard() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: saveDashboard,
    onSuccess: () => client.invalidateQueries({ queryKey: savedKeys.all }),
  });
}

export function useDeleteDashboard() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => fhirClient.delete("Dashboard", id),
    onSuccess: () => client.invalidateQueries({ queryKey: savedKeys.all }),
  });
}

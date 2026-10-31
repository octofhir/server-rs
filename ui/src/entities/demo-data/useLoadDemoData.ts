import { notify } from "@octofhir/ui-kit";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { fhirClient } from "@/shared/api";
import { buildDemoBundle } from "./bundle";

/** Posts the demo clinic as one transaction and refreshes everything that lists data. */
export function useLoadDemoData() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => fhirClient.customRequest({ method: "POST", url: "/fhir", data: buildDemoBundle() }),
    onSuccess: () => {
      void queryClient.invalidateQueries();
      notify({
        theme: "success",
        title: "Demo data loaded",
        content: "30 patients with encounters, vitals and conditions.",
        autoHiding: 4000,
      });
    },
    onError: (error: unknown) =>
      notify({
        theme: "danger",
        title: "Could not load demo data",
        content: error instanceof Error ? error.message : undefined,
      }),
  });
}

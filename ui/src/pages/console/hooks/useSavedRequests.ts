import { notifications } from "@octofhir/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useAuth } from "@/shared/api/hooks/useAuth";
import { savedRequestService } from "../services/savedRequestService";

const KEY = ["console-saved"];

export function useSavedRequests() {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  const userId = user?.sub ?? "anonymous";

  const collectionsQuery = useQuery({
    queryKey: [...KEY, "collections", userId],
    queryFn: () => savedRequestService.listCollections(),
    staleTime: 30000,
  });

  const requestsQuery = useQuery({
    queryKey: [...KEY, "requests", userId],
    queryFn: () => savedRequestService.listRequests(),
    staleTime: 30000,
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: KEY });

  const saveMutation = useMutation({
    mutationFn: savedRequestService.saveRequest,
    onSuccess: () => {
      invalidate();
      notifications.show({ title: "Saved", message: "Request saved", color: "blue" });
    },
  });

  const createCollectionMutation = useMutation({
    mutationFn: ({ name, description }: { name: string; description?: string }) =>
      savedRequestService.createCollection(name, description),
    onSuccess: invalidate,
  });

  const deleteRequestMutation = useMutation({
    mutationFn: (id: string) => savedRequestService.deleteRequest(id),
    onSuccess: invalidate,
  });

  const deleteCollectionMutation = useMutation({
    mutationFn: (id: string) => savedRequestService.deleteCollection(id),
    onSuccess: invalidate,
  });

  return {
    collections: collectionsQuery.data ?? [],
    requests: requestsQuery.data ?? [],
    isLoading: collectionsQuery.isLoading || requestsQuery.isLoading,
    saveRequest: saveMutation.mutateAsync,
    createCollection: createCollectionMutation.mutateAsync,
    deleteRequest: deleteRequestMutation.mutate,
    deleteCollection: deleteCollectionMutation.mutate,
  };
}

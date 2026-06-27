import { notifications } from "@octofhir/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useSyncExternalStore } from "react";
import { useAuth } from "@/shared/api/hooks/useAuth";
import {
  type Environment,
  environmentService,
  getActiveEnvId,
  setActiveEnvId,
  setActiveVariables,
  setEnvironmentUser,
  subscribeActiveEnv,
} from "../services/environmentService";

const KEY = ["console-environments"];

export function useEnvironments() {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  const userId = user?.sub ?? "anonymous";
  setEnvironmentUser(userId);

  // Shared across every hook instance so the page + panel agree on the active env.
  const activeId = useSyncExternalStore(subscribeActiveEnv, getActiveEnvId, getActiveEnvId);

  const envQuery = useQuery({
    queryKey: [...KEY, userId],
    queryFn: () => environmentService.list(),
    staleTime: 30000,
  });

  const environments = envQuery.data ?? [];
  const active = environments.find((e) => e.id === activeId) ?? null;

  // Keep the send-time variable resolver in sync with the active environment.
  // biome-ignore lint/correctness/useExhaustiveDependencies: derived from active env identity
  useEffect(() => {
    const map: Record<string, string> = {};
    if (active) for (const v of active.variables) map[v.key] = v.value;
    setActiveVariables(map);
  }, [active?.id, environments]);

  const setActive = (id: string | null) => setActiveEnvId(id);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: KEY });

  const createMutation = useMutation({
    mutationFn: (name: string) => environmentService.create(name),
    onSuccess: (env) => {
      invalidate();
      setActive(env.id);
      notifications.show({ title: "Environment created", message: env.name, color: "blue" });
    },
  });

  const updateMutation = useMutation({
    mutationFn: (env: Environment) => environmentService.update(env),
    onSuccess: invalidate,
  });

  const removeMutation = useMutation({
    mutationFn: (id: string) => environmentService.remove(id),
    onSuccess: (_d, id) => {
      invalidate();
      if (activeId === id) setActive(null);
    },
  });

  return {
    environments,
    active,
    activeId,
    setActive,
    isLoading: envQuery.isLoading,
    createEnvironment: createMutation.mutateAsync,
    updateEnvironment: updateMutation.mutateAsync,
    removeEnvironment: removeMutation.mutate,
  };
}

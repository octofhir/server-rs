import { notifications } from "@octofhir/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getBundleResources } from "@/shared/api";
import { fhirClient } from "@/shared/api/fhirClient";
import type { FhirResource, UserResource, UserSession } from "@/shared/api/types";

// Filter parameters for user list
export interface UserFilterParams {
  count?: number;
  offset?: number;
  search?: string;
  role?: string;
  status?: "active" | "inactive" | "locked";
  active?: boolean;
}

// Query keys
export const userKeys = {
  all: ["users"] as const,
  lists: () => [...userKeys.all, "list"] as const,
  list: (params: UserFilterParams) => [...userKeys.lists(), params] as const,
  details: () => [...userKeys.all, "detail"] as const,
  detail: (id: string) => [...userKeys.details(), id] as const,
  sessions: (userId: string) => [...userKeys.all, "sessions", userId] as const,
};

// Hooks
export function useUsers(params: UserFilterParams = {}) {
  return useQuery({
    queryKey: userKeys.list(params),
    queryFn: async () => {
      const searchParams: Record<string, string | number> = {};
      if (params.count) searchParams._count = params.count;
      if (params.offset) searchParams._offset = params.offset;
      if (params.search) searchParams.username = params.search;
      if (params.role) searchParams.role = params.role;
      if (params.status) searchParams.status = params.status;
      if (params.active !== undefined) searchParams.active = params.active.toString();

      return fhirClient.search<UserResource>("User", searchParams);
    },
  });
}

export function useUser(id: string | null) {
  return useQuery({
    queryKey: userKeys.detail(id || ""),
    queryFn: async () => {
      if (!id) throw new Error("ID required");
      return fhirClient.read<UserResource>("User", id);
    },
    enabled: !!id,
  });
}

export function useCreateUser() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (user: Partial<UserResource>) =>
      fhirClient.create<UserResource>({ ...user, resourceType: "User" }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: userKeys.lists() });
      notifications.show({
        title: "User created",
        message: "The user has been successfully created.",
        color: "green",
      });
    },
    onError: (error: Error) => {
      notifications.show({
        title: "Failed to create user",
        message: error.message,
        color: "red",
      });
    },
  });
}

export function useUpdateUser() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (user: UserResource) => {
      if (!user.id) throw new Error("User ID required for update");
      return fhirClient.update<UserResource>(user);
    },
    onSuccess: (data) => {
      queryClient.invalidateQueries({ queryKey: userKeys.lists() });
      queryClient.invalidateQueries({ queryKey: userKeys.detail(data.id || "") });
      notifications.show({
        title: "User updated",
        message: "The user has been successfully updated.",
        color: "green",
      });
    },
    onError: (error: Error) => {
      notifications.show({
        title: "Failed to update user",
        message: error.message,
        color: "red",
      });
    },
  });
}

export function useDeleteUser() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (id: string) => {
      await fhirClient.delete("User", id);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: userKeys.lists() });
      notifications.show({
        title: "User deleted",
        message: "The user has been successfully deleted.",
        color: "green",
      });
    },
    onError: (error: Error) => {
      notifications.show({
        title: "Failed to delete user",
        message: error.message,
        color: "red",
      });
    },
  });
}

// Password reset mutation
export function useResetPassword() {
  return useMutation({
    mutationFn: async ({ userId, newPassword }: { userId: string; newPassword: string }) => {
      const response = await fetch(`/User/${userId}/$reset-password`, {
        method: "POST",
        credentials: "include",
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
        },
        body: JSON.stringify({ newPassword }),
      });

      if (!response.ok) {
        const error = await response.json().catch(() => ({ message: response.statusText }));
        throw new Error(error.message || `HTTP ${response.status}`);
      }

      return response.json();
    },
    onSuccess: () => {
      notifications.show({
        title: "Password reset",
        message: "The user's password has been successfully reset.",
        color: "green",
      });
    },
    onError: (error: Error) => {
      notifications.show({
        title: "Failed to reset password",
        message: error.message,
        color: "red",
      });
    },
  });
}

// User sessions - uses FHIR search on AuthSession resource
export function useUserSessions(userId: string | null) {
  return useQuery({
    queryKey: userKeys.sessions(userId || ""),
    queryFn: async () => {
      if (!userId) throw new Error("User ID required");
      const response = await fhirClient.search<FhirResource>("AuthSession", {
        subject: `User/${userId}`,
        status: "active",
      });

      // Transform AuthSession resources to UserSession format
      const sessions: UserSession[] = getBundleResources(response).map((resource) => {
        return {
          id: resource.id ?? "",
          userId: getReferenceId(resource.subject, "User") ?? userId,
          clientId: getReferenceId(resource.client, "Client"),
          clientName: getReferenceDisplay(resource.client),
          ipAddress: getString(resource.ipAddress),
          userAgent: getString(resource.userAgent),
          createdAt: getString(resource.createdAt) ?? resource.meta?.lastUpdated ?? "",
          expiresAt: getString(resource.expiresAt) ?? "",
          lastActivity:
            getString(resource.lastActivityAt) ??
            getString(resource.lastActivity) ??
            resource.meta?.lastUpdated,
        };
      });

      return sessions;
    },
    enabled: !!userId,
  });
}

export function useRevokeSession() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ userId, sessionId }: { userId: string; sessionId: string }) => {
      await fhirClient.delete("AuthSession", sessionId);
      return { userId };
    },
    // Optimistically drop the revoked session so the list updates instantly.
    onMutate: async ({ userId, sessionId }) => {
      const key = userKeys.sessions(userId);
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<UserSession[]>(key);
      queryClient.setQueryData<UserSession[]>(key, (current) =>
        (current ?? []).filter((session) => session.id !== sessionId)
      );
      return { previous };
    },
    onError: (error: Error, variables, context) => {
      // Roll back the optimistic removal on failure.
      if (context?.previous) {
        queryClient.setQueryData(userKeys.sessions(variables.userId), context.previous);
      }
      notifications.show({
        title: "Failed to revoke session",
        message: error.message,
        color: "red",
      });
    },
    onSuccess: () => {
      notifications.show({
        title: "Session revoked",
        message: "The session has been successfully revoked.",
        color: "green",
      });
    },
    // Always re-sync with the server, whether the revoke succeeded or failed.
    onSettled: (_data, _error, variables) => {
      queryClient.invalidateQueries({ queryKey: userKeys.sessions(variables.userId) });
    },
  });
}

// Bulk actions
export function useBulkUpdateUsers() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({
      userIds,
      updates,
    }: {
      userIds: string[];
      updates: Partial<UserResource>;
    }) => {
      const promises = userIds.map(async (id) => {
        // User is an internal resource, so it's at root level
        const user = await fhirClient.read<UserResource>("User", id);
        const updatedUser = { ...user, ...updates };
        return fhirClient.update<UserResource>(updatedUser);
      });
      return Promise.all(promises);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: userKeys.lists() });
      notifications.show({
        title: "Users updated",
        message: "The selected users have been successfully updated.",
        color: "green",
      });
    },
    onError: (error: Error) => {
      notifications.show({
        title: "Failed to update users",
        message: error.message,
        color: "red",
      });
    },
  });
}

// Search for FHIR resources (Practitioner or Patient) by name
export function useSearchResources(resourceType: "Practitioner" | "Patient", search: string) {
  return useQuery({
    queryKey: ["resources", resourceType, search],
    queryFn: async () => {
      if (!search || search.length < 2) {
        return { entry: [] };
      }
      return fhirClient.search(resourceType, {
        name: search,
        _count: 10,
      });
    },
    enabled: search.length >= 2,
  });
}

function getString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function getReferenceId(value: unknown, resourceType: string): string | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const reference = (value as { reference?: unknown }).reference;
  if (typeof reference !== "string") return undefined;
  return reference.replace(`${resourceType}/`, "");
}

function getReferenceDisplay(value: unknown): string | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const display = (value as { display?: unknown }).display;
  return typeof display === "string" ? display : undefined;
}

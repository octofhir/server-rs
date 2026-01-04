import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { fhirClient } from '@/shared/api/fhirClient';
import type { FhirResource } from '@/shared/api/types';

/**
 * AuthSession resource type definition
 * Based on the FHIR StructureDefinition created in the IG
 */
export interface AuthSession extends FhirResource {
  resourceType: 'AuthSession';
  status: 'active' | 'expired' | 'revoked';
  sessionToken: string;
  subject: {
    reference: string; // e.g., "User/uuid"
  };
  deviceName?: string;
  userAgent?: string;
  ipAddress?: string;
  createdAt: string; // ISO datetime
  lastActivityAt?: string; // ISO datetime
  expiresAt: string; // ISO datetime
}

/**
 * Parse AuthSession from FHIR Bundle entry
 */
function parseAuthSession(resource: any): AuthSession {
  return {
    ...resource,
    resourceType: 'AuthSession',
  } as AuthSession;
}

/**
 * Extract user ID from subject reference
 */
export function extractUserId(session: AuthSession): string {
  const match = session.subject.reference.match(/User\/(.+)/);
  return match ? match[1] : '';
}

/**
 * Check if session is the current one based on cookie
 */
export function isCurrentSession(session: AuthSession, cookieToken?: string): boolean {
  return cookieToken ? session.sessionToken === cookieToken : false;
}

/**
 * Get current session token from cookie
 */
export function getCurrentSessionToken(): string | undefined {
  const cookies = document.cookie.split(';');
  const ssoCookie = cookies.find((c) => c.trim().startsWith('octofhir_sso='));
  if (!ssoCookie) return undefined;
  return ssoCookie.split('=')[1]?.trim();
}

/**
 * Hook to fetch active sessions for a user
 */
export function useSessions(userId: string) {
  return useQuery({
    queryKey: ['auth-sessions', userId],
    queryFn: async () => {
      const params: Record<string, string> = {
        subject: `User/${userId}`,
        'expires-at': `gt${new Date().toISOString()}`,
        _sort: '-_lastUpdated',
        status: 'active',
      };

      const response = await fhirClient.search('AuthSession', params);

      if (!response || !response.entry) {
        return [];
      }

      return response.entry
        .filter((entry) => entry.resource)
        .map((entry) => parseAuthSession(entry.resource));
    },
    enabled: Boolean(userId),
    refetchInterval: 30000, // Refresh every 30 seconds
  });
}

/**
 * Hook to revoke a specific session
 */
export function useRevokeSession() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (sessionId: string) => {
      const response = await fhirClient.customRequest({
        method: 'POST',
        url: `/AuthSession/${sessionId}/$revoke`,
        data: {},
      });
      return response.data;
    },
    onSuccess: () => {
      // Invalidate sessions query to refresh the list
      queryClient.invalidateQueries({ queryKey: ['auth-sessions'] });
    },
  });
}

/**
 * Hook to revoke all sessions except current
 */
export function useRevokeAllSessions() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ userId, currentSessionId }: { userId: string; currentSessionId?: string }) => {
      const params: any = {
        subject: `User/${userId}`,
      };

      if (currentSessionId) {
        params.excludeSession = currentSessionId;
      }

      const response = await fhirClient.customRequest({
        method: 'POST',
        url: '/AuthSession/$revoke-all',
        data: params,
      });
      return response.data;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['auth-sessions'] });
    },
  });
}

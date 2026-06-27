import { useEffect } from "react";
import { setHistoryUser } from "@/pages/console/services/historyService";
import { setPreferencesUser } from "../userPreferences";
import { useAuth } from "./useAuth";

/**
 * Keeps the server-backed preference + history layers scoped to the logged-in user.
 * Runs at the app root so the active user is known before feature stores (console
 * prefs, history) are first read.
 */
export function usePreferenceUserSync(): void {
  const { user } = useAuth();
  const userId = user?.sub ?? null;

  useEffect(() => {
    setPreferencesUser(userId);
    setHistoryUser(userId);
  }, [userId]);
}

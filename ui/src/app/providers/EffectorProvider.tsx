import { notifications } from "@mantine/notifications";
import { useUnit } from "effector-react";
import { useEffect } from "react";
import {
  $connectionStatus,
  $systemError,
  getBuildInfoFx,
  getHealthFx,
  getResourceTypesFx,
} from "@/entities/system";
import { $apiBaseUrl, $apiTimeout } from "@/entities/settings/model";
import { fhirClient, serverApi } from "@/shared/api";

interface EffectorProviderProps {
  children: React.ReactNode;
}

export function EffectorProvider({ children }: EffectorProviderProps) {
  const systemError = useUnit($systemError);
  const connectionStatus = useUnit($connectionStatus);
  const [apiBaseUrl, apiTimeout] = useUnit([$apiBaseUrl, $apiTimeout]);

  // Apply settings to API clients when changed
  useEffect(() => {
    try {
      serverApi.setBaseUrl(apiBaseUrl);
      serverApi.setTimeout(apiTimeout);
      fhirClient.setBaseUrl(apiBaseUrl);
      fhirClient.setTimeout(apiTimeout);
    } catch (error) {
      console.warn("Failed to apply API client settings:", error);
    }
  }, [apiBaseUrl, apiTimeout]);

  // Initialize system state on app start
  useEffect(() => {
    const initializeApp = async () => {
      try {
        // Ensure API clients use current settings before initial fetch
        serverApi.setBaseUrl(apiBaseUrl);
        serverApi.setTimeout(apiTimeout);
        fhirClient.setBaseUrl(apiBaseUrl);
        fhirClient.setTimeout(apiTimeout);
        // Load initial system data
        await Promise.allSettled([getHealthFx(), getBuildInfoFx(), getResourceTypesFx()]);
      } catch (error) {
        console.warn("Failed to initialize app state:", error);
      }
    };

    initializeApp();
  }, []);

  // Show notifications for system errors
  useEffect(() => {
    if (systemError) {
      notifications.show({
        title: "System Error",
        message: systemError,
        color: "red",
        autoClose: 5000,
      });
    }
  }, [systemError]);

  // Show connection status notifications
  useEffect(() => {
    if (connectionStatus === "connected") {
      notifications.show({
        title: "Connected",
        message: "Successfully connected to server",
        color: "green",
        autoClose: 3000,
      });
    } else if (connectionStatus === "disconnected") {
      notifications.show({
        title: "Disconnected",
        message: "Lost connection to server",
        color: "red",
        autoClose: false,
      });
    }
  }, [connectionStatus]);

  return <>{children}</>;
}

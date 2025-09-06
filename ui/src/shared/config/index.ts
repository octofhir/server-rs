export const APP_CONFIG = {
  name: "OctoFHIR Server UI",
  version: "1.0.0",
  defaultFhirBaseUrl: window.location.origin,
  healthPollInterval: 15000, // 15 seconds
  maxHistoryItems: 20,
  supportedFhirVersions: ["R4", "R4B", "R5"],
} as const;

export type AppConfig = typeof APP_CONFIG;

export const APP_CONFIG = {
  name: "OctoFHIR Server UI",
  version: "0.1.0",
  healthPollInterval: 15000,
  maxHistoryItems: 20,
  supportedFhirVersions: ["R4", "R4B", "R5"],
  defaultPageSize: 20,
  requestTimeout: 30000,
} as const;

export type AppConfig = typeof APP_CONFIG;

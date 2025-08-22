export interface Environment {
  API_BASE_URL: string;
  FHIR_BASE_URL: string;
  DEV_MODE: boolean;
  DEBUG_LOGGING: boolean;
  ENABLE_MOCK_DATA: boolean;
  ENABLE_ANALYTICS: boolean;
}

const getEnvVar = (key: string, defaultValue?: string): string => {
  const value = import.meta.env[key];
  if (value === undefined && defaultValue === undefined) {
    throw new Error(`Environment variable ${key} is not defined`);
  }
  return value || defaultValue || "";
};

const getBooleanEnvVar = (key: string, defaultValue = false): boolean => {
  const value = import.meta.env[key];
  if (value === undefined) return defaultValue;
  return value.toLowerCase() === "true";
};

export const env: Environment = {
  API_BASE_URL: getEnvVar("VITE_API_BASE_URL", "http://localhost:8080/api"),
  FHIR_BASE_URL: getEnvVar("VITE_FHIR_BASE_URL", "http://localhost:8080/fhir"),
  DEV_MODE: getBooleanEnvVar("VITE_DEV_MODE", true),
  DEBUG_LOGGING: getBooleanEnvVar("VITE_DEBUG_LOGGING", true),
  ENABLE_MOCK_DATA: getBooleanEnvVar("VITE_ENABLE_MOCK_DATA", false),
  ENABLE_ANALYTICS: getBooleanEnvVar("VITE_ENABLE_ANALYTICS", false),
};

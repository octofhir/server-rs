/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_API_BASE_URL: string;
  readonly VITE_FHIR_BASE_URL: string;
  readonly VITE_DEV_MODE: string;
  readonly VITE_DEBUG_LOGGING: string;
  readonly VITE_ENABLE_MOCK_DATA: string;
  readonly VITE_ENABLE_ANALYTICS: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

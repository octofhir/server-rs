import { createSignal, createEffect } from "solid-js";
import { fhirClient } from "@/shared/api";
import { APP_CONFIG } from "@/shared/config";

export type ColorScheme = "light" | "dark" | "system";

// Settings state
const [requestTimeout, setRequestTimeoutInternal] = createSignal(
  Number(localStorage.getItem("octofhir-request-timeout")) || APP_CONFIG.requestTimeout,
);
const [colorScheme, setColorSchemeInternal] = createSignal<ColorScheme>(
  (localStorage.getItem("octofhir-color-scheme") as ColorScheme) || "system",
);
const [pageSize, setPageSizeInternal] = createSignal(
  Number(localStorage.getItem("octofhir-page-size")) || APP_CONFIG.defaultPageSize,
);

// Setters that persist to localStorage
export const setRequestTimeout = (timeout: number) => {
  setRequestTimeoutInternal(timeout);
  localStorage.setItem("octofhir-request-timeout", String(timeout));
  fhirClient.setTimeout(timeout);
};

export const setColorScheme = (scheme: ColorScheme) => {
  setColorSchemeInternal(scheme);
  localStorage.setItem("octofhir-color-scheme", scheme);
};

export const setPageSize = (size: number) => {
  setPageSizeInternal(size);
  localStorage.setItem("octofhir-page-size", String(size));
};

// Initialize FHIR client with stored settings
fhirClient.setTimeout(requestTimeout());

// Exports
export { requestTimeout, colorScheme, pageSize };

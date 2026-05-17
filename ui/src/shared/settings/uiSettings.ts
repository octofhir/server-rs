import { useLocalStorage } from "@octofhir/ui-kit";
import { isRecord } from "@/shared/api/guards";

export interface UiSettings {
	requestTimeoutMs: number;
	skipConsoleValidation: boolean;
	allowAnonymousConsoleRequests: boolean;
	disableAuthAutoLogout: boolean;
}

const DEFAULT_SETTINGS: UiSettings = {
	requestTimeoutMs: 30000,
	skipConsoleValidation: false,
	allowAnonymousConsoleRequests: false,
	disableAuthAutoLogout: false,
};

const STORAGE_KEY = "octofhir-ui-settings";

function isUiSettings(value: unknown): value is UiSettings {
	return (
		isRecord(value) &&
		typeof value.requestTimeoutMs === "number" &&
		typeof value.skipConsoleValidation === "boolean" &&
		typeof value.allowAnonymousConsoleRequests === "boolean" &&
		typeof value.disableAuthAutoLogout === "boolean"
	);
}

export function useUiSettings() {
	return useLocalStorage<UiSettings>({
		key: STORAGE_KEY,
		defaultValue: DEFAULT_SETTINGS,
		validate: isUiSettings,
	});
}

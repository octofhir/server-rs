import { useLocalStorage } from "@mantine/hooks";

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

export function useUiSettings() {
	return useLocalStorage<UiSettings>({
		key: STORAGE_KEY,
		defaultValue: DEFAULT_SETTINGS,
	});
}

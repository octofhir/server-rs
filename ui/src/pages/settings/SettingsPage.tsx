import {
	Text,
	Badge,
	Button,
	NumberInput,
	Select,
	Switch,
	Loader,
	WorkspacePageLayout,
	WorkspacePageSection,
} from "@octofhir/ui-kit";
import { useColorScheme } from "@octofhir/ui-kit";
import { useHealth, useFormatterSettings } from "@/shared/api/hooks";
import { useUiSettings } from "@/shared";
import { FormatterSettings } from "@/shared/settings/FormatterSettings";
import classes from "./SettingsPage.module.css";

type ThemeValue = "light" | "dark" | "auto";

const themeOptions: Array<{ value: ThemeValue; label: string }> = [
	{ value: "light", label: "Light" },
	{ value: "dark", label: "Dark" },
	{ value: "auto", label: "System" },
];

function isThemeValue(value: string | null): value is ThemeValue {
	return value === "light" || value === "dark" || value === "auto";
}

export function SettingsPage() {
	const { data: health, refetch, isRefetching } = useHealth({ refetchInterval: false });
	const { colorScheme, setColorScheme } = useColorScheme();
	const [settings, setSettings] = useUiSettings();
	const {
		config: formatterConfig,
		isLoading: formatterLoading,
		saveConfig: saveFormatterConfig,
	} = useFormatterSettings();

	const statusColor = {
		ok: "primary",
		degraded: "warm",
		down: "fire",
	}[health?.status ?? "down"];

	const handleTestConnection = () => {
		refetch();
	};

	return (
		<WorkspacePageLayout
			title="Settings"
			description="Configure server settings and preferences"
			maxWidth={1120}
		>
			<WorkspacePageSection
				title="Connection"
				description="Server reachability and request behavior."
				actions={
					<Button
						size="sm"
						variant="subtle"
						onClick={handleTestConnection}
						loading={isRefetching}
					>
						Test Connection
					</Button>
				}
			>
				<div className={classes.panel}>
					<div className={classes.connectionRow}>
						<div className={classes.statusRow}>
							<Text variant="body-2" color="secondary">
								Server status
							</Text>
							<Badge color={statusColor} variant="light">
								{health?.status ?? "Unknown"}
							</Badge>
						</div>

						<NumberInput
							label="Request timeout"
							description="Milliseconds before a request is aborted."
							value={settings.requestTimeoutMs}
							onChange={(val) =>
								setSettings((current) => ({
									...current,
									requestTimeoutMs: Number(val) || 30000,
								}))
							}
							min={1000}
							max={120000}
							step={1000}
							className={classes.compactField}
						/>
					</div>
				</div>
			</WorkspacePageSection>

			<WorkspacePageSection
				title="Appearance"
				description="Local display preferences for this browser."
			>
				<div className={classes.panel}>
					<Select
						label="Theme"
						description="Choose your preferred color scheme."
						data={themeOptions}
						value={colorScheme}
						onChange={(val) => {
							if (isThemeValue(val)) {
								setColorScheme(val);
							}
						}}
						className={classes.compactField}
					/>
				</div>
			</WorkspacePageSection>

			<WorkspacePageSection
				title="SQL Formatter"
				description="Formatting options for the DB Console editor."
			>
				<div className={classes.panel}>
					{formatterLoading ? (
						<div className={classes.loadingState}>
							<Loader size="sm" />
							<Text variant="body-2" color="secondary">Loading formatter settings...</Text>
						</div>
					) : (
						<FormatterSettings
							value={formatterConfig}
							onChange={saveFormatterConfig}
						/>
					)}
				</div>
			</WorkspacePageSection>

			<WorkspacePageSection
				title="Console"
				description="Request validation and credential behavior for interactive tools."
			>
				<div className={classes.panel}>
					<div className={classes.switchStack}>
						<Switch
							label="Skip request validation"
							description="Allows sending malformed paths or missing parameters."
							checked={settings.skipConsoleValidation}
							onChange={(event) =>
								setSettings((current) => ({
									...current,
									skipConsoleValidation: event,
								}))
							}
						/>
						<Switch
							label="Allow anonymous REST console requests"
							description="Send requests without cookies/credentials."
							checked={settings.allowAnonymousConsoleRequests}
							onChange={(event) =>
								setSettings((current) => ({
									...current,
									allowAnonymousConsoleRequests: event,
								}))
							}
						/>
					</div>
				</div>
			</WorkspacePageSection>

			<WorkspacePageSection
				title="Authentication"
				description="Session handling behavior in the UI."
			>
				<div className={classes.panel}>
					<Switch
						label="Disable auto-logout on 401/403"
						description="Keeps the UI state when the session expires."
						checked={settings.disableAuthAutoLogout}
						onChange={(event) =>
							setSettings((current) => ({
								...current,
								disableAuthAutoLogout: event,
							}))
						}
					/>
				</div>
			</WorkspacePageSection>
		</WorkspacePageLayout>
	);
}

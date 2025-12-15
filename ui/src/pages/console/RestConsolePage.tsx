import { useMemo, useState } from "react";
import { Helmet } from "react-helmet-async";
import {
	Alert,
	Badge,
	Button,
	Grid,
	Group,
	Skeleton,
	Stack,
	Text,
	Title,
} from "@mantine/core";
import { useHotkeys } from "@mantine/hooks";
import { IconAlertCircle, IconRefresh, IconHistory } from "@tabler/icons-react";
import { ConsolePanel } from "./components/ConsolePanel";
import { MethodControl } from "./components/MethodControl";
import { ModeControl } from "./components/ModeControl";
import { UnifiedPathBuilder } from "./components/UnifiedPathBuilder";
import { RawPathInput } from "./components/RawPathInput";
import { RequestBuilderAccordion } from "./components/RequestBuilderAccordion";
import { ResponseViewer } from "./components/ResponseViewer";
import { HistoryPanel } from "./components/HistoryPanel";
import { CommandPalette } from "./components/CommandPalette";
import { useRestConsoleMeta } from "./hooks/useRestConsoleMeta";
import { useSendConsoleRequest } from "./hooks/useSendConsoleRequest";
import { useConsoleStore } from "./state/consoleStore";

export function RestConsolePage() {
	const {
		data: meta,
		isPending,
		isError,
		refetch,
		resourceTypes,
		allSuggestions,
		searchParamsByResource,
	} = useRestConsoleMeta();

	const baseUrl = useResolvedBaseUrl(meta?.base_path);

	// Console state
	const method = useConsoleStore((state) => state.method);
	const mode = useConsoleStore((state) => state.mode);
	const rawPath = useConsoleStore((state) => state.rawPath);
	const body = useConsoleStore((state) => state.body);
	const customHeaders = useConsoleStore((state) => state.customHeaders);
	const setCommandPaletteOpen = useConsoleStore((state) => state.setCommandPaletteOpen);

	// Send request mutation
	const sendMutation = useSendConsoleRequest();

	// History drawer state
	const [historyOpened, setHistoryOpened] = useState(false);

	const handleSend = () => {
		sendMutation.mutate({
			method,
			path: rawPath,
			body,
			headers: customHeaders,
		});
	};

	// Keyboard shortcuts
	useHotkeys([
		["mod+K", (e) => {
			e.preventDefault();
			console.log("[RestConsolePage] Cmd+K pressed, opening palette");
			setCommandPaletteOpen(true);
		}],
		["mod+Enter", handleSend]
	]);

	return (
		<>
			<Helmet>
				<title>REST Console</title>
			</Helmet>
			<Stack gap="lg">
				<header>
					<Stack gap="xs">
						<Group justify="space-between">
							<Title order={2}>REST Console</Title>
							<Button
								variant="light"
								leftSection={<IconHistory size={16} />}
								onClick={() => setHistoryOpened(true)}
								size="sm"
							>
								History
							</Button>
						</Group>
						<Group gap="sm">
							<Text c="dimmed" size="sm">
								Build and execute FHIR REST requests with smart autocomplete.
							</Text>
							<Badge variant="outline" color="gray">
								{baseUrl}
							</Badge>
						</Group>
					</Stack>
				</header>

				{isError ? (
					<Alert
						icon={<IconAlertCircle size={16} />}
						color="red"
						variant="light"
						title="Unable to load metadata"
						withCloseButton={false}
					>
						<Group gap="sm" align="flex-start">
							<Text size="sm">
								The REST console metadata endpoint is unavailable. Retry once the server is
								reachable.
							</Text>
							<Button
								leftSection={<IconRefresh size={16} />}
								variant="light"
								onClick={() => refetch()}
								size="xs"
							>
								Try again
							</Button>
						</Group>
					</Alert>
				) : null}

				<Grid gutter="lg">
					<Grid.Col span={{ base: 12, md: 8 }}>
						<ConsolePanel
							title="Request Builder"
							subtitle={mode === "smart" ? "Build your FHIR request with autocomplete" : "Enter raw path manually"}
						>
							<Stack gap="md">
								<Group justify="space-between" align="flex-start">
									<Group>
										<MethodControl />
										<ModeControl />
									</Group>
									<Button
										size="sm"
										onClick={handleSend}
										loading={sendMutation.isPending}
										disabled={!rawPath}
									>
										Send Request
									</Button>
								</Group>

								{mode === "smart" ? (
									<UnifiedPathBuilder
										allSuggestions={allSuggestions}
										searchParamsByResource={searchParamsByResource}
										isLoading={isPending}
									/>
								) : (
									<RawPathInput />
								)}

								<RequestBuilderAccordion
									searchParamsByResource={searchParamsByResource}
									hideQuery
								/>
							</Stack>
						</ConsolePanel>
					</Grid.Col>

					<Grid.Col span={{ base: 12, md: 4 }}>
						<ConsolePanel
							title="Response"
							subtitle={sendMutation.isPending ? "Executing..." : "Request results"}
						>
							<ResponseViewer
								response={sendMutation.data}
								isLoading={sendMutation.isPending}
							/>
						</ConsolePanel>

						{!isPending && resourceTypes.length > 0 ? (
							<ConsolePanel title="Server Info" subtitle="Loaded from metadata">
								<Stack gap="xs">
									<Group justify="space-between">
										<Text size="sm" fw={500}>
											Resource Types
										</Text>
										<Badge variant="light">{resourceTypes.length}</Badge>
									</Group>
									<Group justify="space-between">
										<Text size="sm" fw={500}>
											FHIR Version
										</Text>
										<Badge variant="light">{meta?.fhir_version || "R4B"}</Badge>
									</Group>
									<Text size="xs" c="dimmed">
										Search parameters loaded from{" "}
										{Object.keys(searchParamsByResource).length} resources
									</Text>
								</Stack>
							</ConsolePanel>
						) : null}
					</Grid.Col>
				</Grid>
			</Stack>

			<CommandPalette />
			<HistoryPanel opened={historyOpened} onClose={() => setHistoryOpened(false)} />
		</>
	);
}

function useResolvedBaseUrl(basePath?: string) {
	return useMemo(() => {
		const path = basePath ?? "/fhir";
		if (typeof window === "undefined") {
			return path;
		}
		return `${window.location.origin}${path}`;
	}, [basePath]);
}

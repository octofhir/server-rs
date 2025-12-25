import { useCallback, useMemo, useState } from "react";
import { useUnit } from "effector-react";
import { Helmet } from "react-helmet-async";
import {
	Alert,
	Badge,
	Button,
	Grid,
	Group,
	Stack,
	Text,
	Title,
	Box,
} from "@/shared/ui";
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
import {
	$body,
	$customHeaders,
	$method,
	$mode,
	$rawPath,
	setCommandPaletteOpen,
} from "./state/consoleStore";

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
	const {
		method,
		mode,
		rawPath,
		body,
		customHeaders,
		setCommandPaletteOpen: openPalette,
	} = useUnit({
		method: $method,
		mode: $mode,
		rawPath: $rawPath,
		body: $body,
		customHeaders: $customHeaders,
		setCommandPaletteOpen,
	});

	// Send request mutation
	const sendMutation = useSendConsoleRequest();

	// History drawer state
	const [historyOpened, setHistoryOpened] = useState(false);

	const handleSend = useCallback(() => {
		sendMutation.mutate({
			method,
			path: rawPath,
			body,
			headers: customHeaders,
		});
	}, [sendMutation, method, rawPath, body, customHeaders]);

	const handleOpenPalette = useCallback(
		(e: KeyboardEvent) => {
			e.preventDefault();
			openPalette(true);
		},
		[openPalette],
	);

	// Keyboard shortcuts - memoized to prevent infinite re-renders
	const hotkeys = useMemo(
		() =>
			[
				["mod+K", handleOpenPalette],
				["mod+Enter", handleSend],
			] as const,
		[handleOpenPalette, handleSend],
	);

	useHotkeys(hotkeys);

	return (
		<Box className="page-enter" p="xl">
			<Helmet>
				<title>REST Console</title>
			</Helmet>

			<Stack gap="xl">
				<Box>
					<Group justify="space-between" align="flex-end">
						<Box>
							<Title
								order={1}
								style={{ letterSpacing: "-0.03em", fontWeight: 700 }}
							>
								REST Console
							</Title>
							<Group gap="sm" mt={4}>
								<Text c="dimmed" size="lg">
									Build and execute FHIR requests with smart autocomplete
								</Text>
								<Badge variant="dot" color="primary" radius="sm">
									{baseUrl}
								</Badge>
							</Group>
						</Box>
						<Button
							variant="light"
							radius="md"
							leftSection={<IconHistory size={18} />}
							onClick={() => setHistoryOpened(true)}
							size="md"
						>
							History
						</Button>
					</Group>
				</Box>

				{isError ? (
					<Alert
						icon={<IconAlertCircle size={20} />}
						color="red"
						radius="md"
						variant="light"
						title="Connection Failed"
					>
						<Group gap="sm">
							<Text size="sm">
								The metadata endpoint is unavailable. Please check if the server
								is running.
							</Text>
							<Button
								leftSection={<IconRefresh size={14} />}
								variant="subtle"
								onClick={() => refetch()}
								size="xs"
								color="red"
							>
								Retry Connection
							</Button>
						</Group>
					</Alert>
				) : null}

				<Grid gutter="xl">
					<Grid.Col span={{ base: 12, lg: 8 }}>
						<Box
							style={{
								display: "flex",
								flexDirection: "column",
								gap: "var(--mantine-spacing-md)",
								height: "100%",
							}}
						>
							<ConsolePanel
								title="Request Builder"
								subtitle={
									mode === "smart"
										? "IDE-grade FHIR autocomplete"
										: "Manual path override"
								}
							>
								<Stack gap="xl">
									<Group justify="space-between" align="center">
										<Group gap="sm">
											<MethodControl />
											<Box
												style={{
													width: 1,
													height: 24,
													backgroundColor: "var(--app-border-subtle)",
												}}
											/>
											<ModeControl />
										</Group>
										<Button
											size="md"
											radius="md"
											variant="filled"
											onClick={handleSend}
											loading={sendMutation.isPending}
											disabled={!rawPath}
											style={{
												boxShadow:
													"0 8px 16px var(--mantine-color-primary-light-hover)",
												paddingLeft: 24,
												paddingRight: 24,
											}}
										>
											Send Request
										</Button>
									</Group>

									<Box
										p="md"
										style={{
											backgroundColor: "var(--app-surface-2)",
											borderRadius: "var(--mantine-radius-md)",
											border: "1px solid var(--app-border-subtle)",
										}}
									>
										{mode === "smart" ? (
											<UnifiedPathBuilder
												allSuggestions={allSuggestions}
												searchParamsByResource={searchParamsByResource}
												isLoading={isPending}
											/>
										) : (
											<RawPathInput />
										)}
									</Box>

									<RequestBuilderAccordion
										searchParamsByResource={searchParamsByResource}
										hideQuery
									/>
								</Stack>
							</ConsolePanel>
						</Box>
					</Grid.Col>

					<Grid.Col span={{ base: 12, lg: 4 }}>
						<Stack gap="xl">
							<ConsolePanel
								title="Response"
								subtitle={
									sendMutation.isPending
										? "Executing..."
										: "Instant feedback loop"
								}
							>
								<ResponseViewer
									response={sendMutation.data}
									isLoading={sendMutation.isPending}
								/>
							</ConsolePanel>

							{!isPending && resourceTypes.length > 0 ? (
								<ConsolePanel
									title="Server Metadata"
									subtitle="Live endpoint info"
								>
									<Stack gap="md">
										<Box
											p="md"
											style={{
												backgroundColor: "var(--app-surface-2)",
												borderRadius: "var(--mantine-radius-md)",
												border: "1px solid var(--app-border-subtle)",
											}}
										>
											<Group justify="space-between" mb="xs">
												<Text size="sm" fw={600} c="dimmed">
													RESOURCE TYPES
												</Text>
												<Badge variant="light" color="primary" radius="sm">
													{resourceTypes.length}
												</Badge>
											</Group>
											<Group justify="space-between">
												<Text size="sm" fw={600} c="dimmed">
													FHIR VERSION
												</Text>
												<Badge variant="light" color="warm" radius="sm">
													{meta?.fhir_version || "R4B"}
												</Badge>
											</Group>
										</Box>

										<Group gap={6} align="center">
											<IconAlertCircle
												size={14}
												style={{ color: "var(--app-accent-primary)" }}
											/>
											<Text size="xs" c="dimmed">
												Smart index contains{" "}
												{Object.keys(searchParamsByResource).length} cached
												resources
											</Text>
										</Group>
									</Stack>
								</ConsolePanel>
							) : null}
						</Stack>
					</Grid.Col>
				</Grid>
			</Stack>

			<CommandPalette />
			<HistoryPanel
				opened={historyOpened}
				onClose={() => setHistoryOpened(false)}
			/>
		</Box>
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

import { Alert, Badge, Box, Flex, Switch, Text } from "@/shared/ui";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { useState } from "react";

import { CircleInfo } from "@gravity-ui/icons";
import { LogFilters } from "./LogFilters";
import { LogStream } from "./LogStream";
import { useLogStream } from "./useLogStream";
import classes from "./LogsViewerPage.module.css";

export function LogsViewerPage() {
	const [demoMode, setDemoMode] = useState(false);

	const {
		logs,
		isConnected,
		isPaused,
		filters,
		connectionError,
		pause,
		resume,
		clear,
		setFilters,
		exportLogs,
	} = useLogStream({
		maxEntries: 1000,
		demoMode,
		demoInterval: 1500,
	});

	return (
		<WorkspacePageLayout
			title="System Logs"
			description="Real-time server activity and diagnostics"
			className="page-enter"
			bodyClassName={classes.body}
			contentClassName={classes.container}
			actions={
				<Flex gap="2" alignItems="center">
					<Switch
						content="Demo Mode"
						checked={demoMode}
						onUpdate={(checked) => setDemoMode(checked)}
					/>
					<Badge
						theme={isConnected ? "success" : "normal"}
					>
						{isConnected ? "Connected" : "Disconnected"}
					</Badge>
					{isPaused && (
						<Badge theme="warning">
							Paused
						</Badge>
					)}
				</Flex>
			}
		>
			<Flex direction="column" gap="0" className={classes.stack}>
				{demoMode && (
					<Alert
						icon={<CircleInfo size={16} />}
						theme="info"
						className={classes.demoAlert}
					>
						<Text variant="body-1">
							<strong>Demo Mode:</strong> Displaying simulated log data. Disable demo mode to connect to the server WebSocket.
						</Text>
					</Alert>
				)}

				<LogFilters
					filters={filters}
					isPaused={isPaused}
					logCount={logs.length}
					onFiltersChange={setFilters}
					onPause={pause}
					onResume={resume}
					onClear={clear}
					onExport={exportLogs}
				/>

				<Box className={classes.streamContainer}>
					<LogStream
						logs={logs}
						isConnected={isConnected}
						isPaused={isPaused}
						connectionError={connectionError}
						autoScroll={true}
					/>
				</Box>
			</Flex>
		</WorkspacePageLayout>
	);
}

import { Alert, StatusBadge, Switch, WorkspacePageLayout } from "@octofhir/ui-kit";
import { useState } from "react";

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
				<div className={classes.actions}>
					<Switch content="Demo Mode" checked={demoMode} onUpdate={setDemoMode} />
					<StatusBadge tone={isConnected ? "success" : "neutral"}>
						{isConnected ? "Connected" : "Disconnected"}
					</StatusBadge>
					{isPaused && <StatusBadge tone="warning">Paused</StatusBadge>}
				</div>
			}
		>
			<div className={classes.stack}>
				{demoMode && (
					<Alert
						theme="info"
						title="Demo Mode"
						message="Displaying simulated log data. Disable demo mode to connect to the server WebSocket."
						className={classes.demoAlert}
					/>
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

				<div className={classes.streamContainer}>
					<LogStream
						logs={logs}
						isConnected={isConnected}
						isPaused={isPaused}
						connectionError={connectionError}
						autoScroll={true}
					/>
				</div>
			</div>
		</WorkspacePageLayout>
	);
}

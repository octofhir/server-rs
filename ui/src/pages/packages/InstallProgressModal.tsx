import { useMemo } from "react";
import {
	Modal,
	Stack,
	Text,
	Progress,
	Group,
	Badge,
	ThemeIcon,
	Paper,
	ScrollArea,
	Button,
	Alert,
} from "@/shared/ui";
import {
	IconCheck,
	IconDownload,
	IconPackage,
	IconAlertCircle,
	IconLoader,
	IconDatabase,
	IconArchive,
} from "@tabler/icons-react";
import type { InstallEvent } from "@/shared/api/types";

interface InstallProgressModalProps {
	opened: boolean;
	onClose: () => void;
	events: InstallEvent[];
	isInstalling: boolean;
	error: Error | null;
	packageName: string;
	packageVersion: string;
}

interface PackageProgress {
	name: string;
	version: string;
	status: "pending" | "downloading" | "extracting" | "indexing" | "completed" | "skipped" | "error";
	downloadPercent: number;
	resourceCount?: number;
	errorMessage?: string;
	skipReason?: string;
}

function getStatusIcon(status: PackageProgress["status"]) {
	switch (status) {
		case "downloading":
			return <IconDownload size={16} />;
		case "extracting":
			return <IconArchive size={16} />;
		case "indexing":
			return <IconDatabase size={16} />;
		case "completed":
			return <IconCheck size={16} />;
		case "skipped":
			return <IconCheck size={16} />;
		case "error":
			return <IconAlertCircle size={16} />;
		default:
			return <IconLoader size={16} className="animate-spin" />;
	}
}

function getStatusColor(status: PackageProgress["status"]) {
	switch (status) {
		case "completed":
			return "primary";
		case "skipped":
			return "deep";
		case "error":
			return "fire";
		case "downloading":
		case "extracting":
		case "indexing":
			return "primary";
		default:
			return "deep";
	}
}

function getStatusLabel(status: PackageProgress["status"]) {
	switch (status) {
		case "downloading":
			return "Downloading";
		case "extracting":
			return "Extracting";
		case "indexing":
			return "Indexing";
		case "completed":
			return "Installed";
		case "skipped":
			return "Skipped";
		case "error":
			return "Failed";
		default:
			return "Pending";
	}
}

function PackageProgressItem({ pkg }: { pkg: PackageProgress }) {
	const isActive = ["downloading", "extracting", "indexing"].includes(pkg.status);

	return (
		<Paper p="sm" radius="sm" style={{ backgroundColor: "var(--octo-surface-2)" }}>
			<Group justify="space-between" mb={isActive ? "xs" : 0}>
				<Group gap="xs">
					<ThemeIcon size="sm" variant="light" color={getStatusColor(pkg.status)}>
						{getStatusIcon(pkg.status)}
					</ThemeIcon>
					<Text size="sm" fw={500}>
						{pkg.name}
					</Text>
					<Badge size="xs" variant="outline">
						{pkg.version}
					</Badge>
				</Group>
				<Badge size="sm" color={getStatusColor(pkg.status)} variant="light">
					{getStatusLabel(pkg.status)}
				</Badge>
			</Group>

			{pkg.status === "downloading" && (
				<Progress value={pkg.downloadPercent} size="sm" animated striped />
			)}

			{pkg.status === "completed" && pkg.resourceCount !== undefined && (
				<Text size="xs" c="dimmed" mt="xs">
					{pkg.resourceCount} resources indexed
				</Text>
			)}

			{pkg.status === "skipped" && pkg.skipReason && (
				<Text size="xs" c="dimmed" mt="xs">
					{pkg.skipReason}
				</Text>
			)}

			{pkg.status === "error" && pkg.errorMessage && (
				<Text size="xs" c="fire" mt="xs">
					{pkg.errorMessage}
				</Text>
			)}
		</Paper>
	);
}

export function InstallProgressModal({
	opened,
	onClose,
	events,
	isInstalling,
	error,
	packageName,
	packageVersion,
}: InstallProgressModalProps) {
	// Build package progress state from events
	const { packages, totalPackages, completedPackages, overallProgress, isCompleted, completionStats } =
		useMemo(() => {
			const pkgMap = new Map<string, PackageProgress>();
			let total = 0;
			let completed = 0;
			let totalResources = 0;
			let durationMs = 0;
			let hasCompleted = false;

			for (const event of events) {
				switch (event.type) {
					case "started":
						total = event.total_packages;
						break;
					case "dependencies_resolved":
						for (const pkgStr of event.packages) {
							const [name, version] = pkgStr.split("@");
							if (name && version) {
								pkgMap.set(pkgStr, {
									name,
									version,
									status: "pending",
									downloadPercent: 0,
								});
							}
						}
						break;
					case "download_started":
						{
							const key = `${event.package}@${event.version}`;
							pkgMap.set(key, {
								name: event.package,
								version: event.version,
								status: "downloading",
								downloadPercent: 0,
							});
						}
						break;
					case "download_progress":
						{
							const key = `${event.package}@${event.version}`;
							const existing = pkgMap.get(key);
							if (existing) {
								existing.downloadPercent = event.percent;
							}
						}
						break;
					case "download_completed":
						{
							const key = `${event.package}@${event.version}`;
							const existing = pkgMap.get(key);
							if (existing) {
								existing.downloadPercent = 100;
							}
						}
						break;
					case "extracting":
						{
							const key = `${event.package}@${event.version}`;
							const existing = pkgMap.get(key);
							if (existing) {
								existing.status = "extracting";
							}
						}
						break;
					case "indexing":
						{
							const key = `${event.package}@${event.version}`;
							const existing = pkgMap.get(key);
							if (existing) {
								existing.status = "indexing";
							}
						}
						break;
					case "package_installed":
						{
							const key = `${event.package}@${event.version}`;
							const existing = pkgMap.get(key);
							if (existing) {
								existing.status = "completed";
								existing.resourceCount = event.resource_count;
							}
							completed++;
						}
						break;
					case "skipped":
						{
							const key = `${event.package}@${event.version}`;
							pkgMap.set(key, {
								name: event.package,
								version: event.version,
								status: "skipped",
								downloadPercent: 100,
								skipReason: event.reason,
							});
							completed++;
						}
						break;
					case "error":
						if (event.package && event.version) {
							const key = `${event.package}@${event.version}`;
							const existing = pkgMap.get(key);
							if (existing) {
								existing.status = "error";
								existing.errorMessage = event.message;
							}
						}
						break;
					case "completed":
						hasCompleted = true;
						totalResources = event.total_resources;
						durationMs = event.duration_ms;
						break;
				}
			}

			const overallProg = total > 0 ? (completed / total) * 100 : 0;

			return {
				packages: Array.from(pkgMap.values()),
				totalPackages: total,
				completedPackages: completed,
				overallProgress: overallProg,
				isCompleted: hasCompleted,
				completionStats: { totalResources, durationMs },
			};
		}, [events]);

	const latestEvent = events[events.length - 1];
	const statusMessage = useMemo(() => {
		if (!latestEvent) return "Initializing...";

		switch (latestEvent.type) {
			case "resolving_dependencies":
				return `Resolving dependencies for ${latestEvent.package}...`;
			case "dependencies_resolved":
				return `Found ${latestEvent.packages.length} packages to install`;
			case "download_started":
				return `Downloading ${latestEvent.package}@${latestEvent.version}...`;
			case "download_progress":
				return `Downloading ${latestEvent.package}: ${latestEvent.percent}%`;
			case "extracting":
				return `Extracting ${latestEvent.package}...`;
			case "indexing":
				return `Indexing ${latestEvent.package}...`;
			case "package_installed":
				return `Installed ${latestEvent.package} (${latestEvent.resource_count} resources)`;
			case "completed":
				return `Installation complete! ${latestEvent.total_installed} packages, ${latestEvent.total_resources} resources`;
			case "error":
				return `Error: ${latestEvent.message}`;
			case "skipped":
				return `Skipped ${latestEvent.package}: ${latestEvent.reason}`;
			default:
				return "Processing...";
		}
	}, [latestEvent]);

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={
				<Group gap="xs">
					<ThemeIcon size="md" variant="light" color="warm">
						<IconPackage size={16} />
					</ThemeIcon>
					<Text fw={500}>
						Installing {packageName}@{packageVersion}
					</Text>
				</Group>
			}
			size="lg"
			closeOnClickOutside={!isInstalling}
			closeOnEscape={!isInstalling}
			withCloseButton={!isInstalling}
			styles={{ body: { backgroundColor: "var(--octo-surface-1)" } }}
		>
			<Stack gap="md">
				{/* Overall progress */}
				<div>
					<Group justify="space-between" mb="xs">
						<Text size="sm" c="dimmed">
							{statusMessage}
						</Text>
						{totalPackages > 0 && (
							<Text size="sm" c="dimmed">
								{completedPackages}/{totalPackages}
							</Text>
						)}
					</Group>
					<Progress value={overallProgress} size="lg" animated={isInstalling} striped={isInstalling} />
				</div>

				{/* Package list */}
				{packages.length > 0 && (
					<ScrollArea.Autosize mah={300}>
						<Stack gap="xs">
							{packages.map((pkg) => (
								<PackageProgressItem key={`${pkg.name}@${pkg.version}`} pkg={pkg} />
							))}
						</Stack>
					</ScrollArea.Autosize>
				)}

				{/* Error display */}
				{error && (
					<Alert icon={<IconAlertCircle size={16} />} color="fire" variant="light">
						{error.message}
					</Alert>
				)}

				{/* Completion stats */}
				{isCompleted && (
					<Alert icon={<IconCheck size={16} />} color="primary" variant="light">
						Successfully installed {completedPackages} packages with {completionStats.totalResources}{" "}
						resources in {(completionStats.durationMs / 1000).toFixed(1)}s
					</Alert>
				)}

				{/* Actions */}
				<Group justify="flex-end">
					{isInstalling ? (
						<Button variant="default" disabled>
							Installing...
						</Button>
					) : (
						<Button onClick={onClose}>{isCompleted || error ? "Close" : "Cancel"}</Button>
					)}
				</Group>
			</Stack>
		</Modal>
	);
}

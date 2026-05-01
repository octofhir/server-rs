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
	Check,
	ArrowDownToLine,
	Box,
	CircleExclamation,
	Pulse,
	Database,
	Archive,
} from "@gravity-ui/icons";
import {
	buildFhirPackageInstallProgress,
	getFhirPackageInstallStatusMessage,
	getFhirPackageInstallStatusView,
	type FhirPackageInstallStatus,
	type FhirPackageProgress,
} from "@/entities/fhir-package";
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

function getStatusIcon(status: FhirPackageInstallStatus) {
	switch (status) {
		case "downloading":
			return <ArrowDownToLine size={16} />;
		case "extracting":
			return <Archive size={16} />;
		case "indexing":
			return <Database size={16} />;
		case "completed":
			return <Check size={16} />;
		case "skipped":
			return <Check size={16} />;
		case "error":
			return <CircleExclamation size={16} />;
		default:
			return <Pulse size={16} className="animate-spin" />;
	}
}

function PackageProgressItem({ pkg }: { pkg: FhirPackageProgress }) {
	const isActive = ["downloading", "extracting", "indexing"].includes(pkg.status);
	const statusView = getFhirPackageInstallStatusView(pkg.status);

	return (
		<Paper p="sm" radius="sm" style={{ backgroundColor: "var(--octo-surface-2)" }}>
			<Group justify="space-between" mb={isActive ? "xs" : 0}>
				<Group gap="xs">
					<ThemeIcon size="sm" variant="light" color={statusView.color}>
						{getStatusIcon(pkg.status)}
					</ThemeIcon>
					<Text size="sm" fw={500}>
						{pkg.name}
					</Text>
					<Badge size="xs" variant="outline">
						{pkg.version}
					</Badge>
				</Group>
				<Badge size="sm" color={statusView.color} variant="light">
					{statusView.label}
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
	const { packages, totalPackages, completedPackages, overallProgress, isCompleted, completionStats } =
		useMemo(() => buildFhirPackageInstallProgress(events), [events]);

	const latestEvent = events[events.length - 1];
	const statusMessage = useMemo(
		() => getFhirPackageInstallStatusMessage(latestEvent),
		[latestEvent],
	);

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={
				<Group gap="xs">
					<ThemeIcon size="md" variant="light" color="warm">
						<Box size={16} />
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
					<Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
						{error.message}
					</Alert>
				)}

				{/* Completion stats */}
				{isCompleted && (
					<Alert icon={<Check size={16} />} color="primary" variant="light">
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

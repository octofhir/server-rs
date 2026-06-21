import { useMemo } from "react";
import {
	Modal,
	Text,
	Progress,
	Badge,
	ThemeIcon,
	ScrollArea,
	Button,
	Alert,
} from "@octofhir/ui-kit";
import { Check, ArrowDownToLine, CircleAlert as CircleExclamation, Activity as Pulse, Database, Archive } from "lucide-react";
import {
	buildFhirPackageInstallProgress,
	getFhirPackageInstallStatusMessage,
	getFhirPackageInstallStatusView,
	type FhirPackageInstallStatus,
	type FhirPackageProgress,
} from "@/entities/fhir-package";
import type { InstallEvent } from "@/shared/api/types";
import classes from "./InstallProgressModal.module.css";

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
		<div className={classes.packageItem}>
			<div className={isActive ? classes.packageHeaderActive : classes.packageHeader}>
				<div className={classes.packageIdentity}>
					<ThemeIcon size="sm" variant="light" color={statusView.color}>
						{getStatusIcon(pkg.status)}
					</ThemeIcon>
					<Text size="sm" fw={500}>
						{pkg.name}
					</Text>
					<Badge size="xs" variant="outline">
						{pkg.version}
					</Badge>
				</div>
				<Badge size="sm" color={statusView.color} variant="light">
					{statusView.label}
				</Badge>
			</div>

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
		</div>
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
				<div className={classes.modalTitle}>
					<ThemeIcon size="md" variant="light" color="warm">
						<Archive size={16} />
					</ThemeIcon>
					<Text fw={500}>
						Installing {packageName}@{packageVersion}
					</Text>
				</div>
			}
			size="lg"
			closeOnClickOutside={!isInstalling}
			closeOnEscape={!isInstalling}
			withCloseButton={!isInstalling}
			styles={{ body: { backgroundColor: "var(--octo-surface-1)" } }}
		>
			<div className={classes.content}>
				{/* Overall progress */}
				<div>
					<div className={classes.progressHeader}>
						<Text size="sm" c="dimmed">
							{statusMessage}
						</Text>
						{totalPackages > 0 && (
							<Text size="sm" c="dimmed">
								{completedPackages}/{totalPackages}
							</Text>
						)}
					</div>
					<Progress value={overallProgress} size="lg" animated={isInstalling} striped={isInstalling} />
				</div>

				{/* Package list */}
				{packages.length > 0 && (
					<ScrollArea.Autosize mah={300}>
						<div className={classes.packageList}>
							{packages.map((pkg) => (
								<PackageProgressItem key={`${pkg.name}@${pkg.version}`} pkg={pkg} />
							))}
						</div>
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
				<div className={classes.actions}>
					{isInstalling ? (
						<Button variant="default" disabled>
							Installing...
						</Button>
					) : (
						<Button onClick={onClose}>{isCompleted || error ? "Close" : "Cancel"}</Button>
					)}
				</div>
			</div>
		</Modal>
	);
}

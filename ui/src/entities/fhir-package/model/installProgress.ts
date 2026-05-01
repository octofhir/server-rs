import type { InstallEvent } from "@/shared/api/types";

export type FhirPackageInstallStatus =
	| "pending"
	| "downloading"
	| "extracting"
	| "indexing"
	| "completed"
	| "skipped"
	| "error";

export interface FhirPackageProgress {
	name: string;
	version: string;
	status: FhirPackageInstallStatus;
	downloadPercent: number;
	resourceCount?: number;
	errorMessage?: string;
	skipReason?: string;
}

export interface FhirPackageInstallProgressState {
	packages: FhirPackageProgress[];
	totalPackages: number;
	completedPackages: number;
	overallProgress: number;
	isCompleted: boolean;
	completionStats: {
		totalResources: number;
		durationMs: number;
	};
}

export interface FhirPackageInstallStatusView {
	label: string;
	color: string;
}

export function getFhirPackageInstallStatusView(
	status: FhirPackageInstallStatus,
): FhirPackageInstallStatusView {
	switch (status) {
		case "downloading":
			return { label: "Downloading", color: "primary" };
		case "extracting":
			return { label: "Extracting", color: "primary" };
		case "indexing":
			return { label: "Indexing", color: "primary" };
		case "completed":
			return { label: "Installed", color: "primary" };
		case "skipped":
			return { label: "Skipped", color: "deep" };
		case "error":
			return { label: "Failed", color: "fire" };
		default:
			return { label: "Pending", color: "deep" };
	}
}

export function getFhirPackageInstallStatusMessage(event: InstallEvent | undefined): string {
	if (!event) return "Initializing...";

	switch (event.type) {
		case "resolving_dependencies":
			return `Resolving dependencies for ${event.package}...`;
		case "dependencies_resolved":
			return `Found ${event.packages.length} packages to install`;
		case "download_started":
			return `Downloading ${event.package}@${event.version}...`;
		case "download_progress":
			return `Downloading ${event.package}: ${event.percent}%`;
		case "download_completed":
			return `Downloaded ${event.package}`;
		case "extracting":
			return `Extracting ${event.package}...`;
		case "extracted":
			return `Extracted ${event.package} (${event.resource_count} resources)`;
		case "indexing":
			return `Indexing ${event.package}...`;
		case "package_installed":
			return `Installed ${event.package} (${event.resource_count} resources)`;
		case "completed":
			return `Installation complete! ${event.total_installed} packages, ${event.total_resources} resources`;
		case "error":
			return `Error: ${event.message}`;
		case "skipped":
			return `Skipped ${event.package}: ${event.reason}`;
		default:
			return "Processing...";
	}
}

export function buildFhirPackageInstallProgress(
	events: InstallEvent[],
): FhirPackageInstallProgressState {
	const packageMap = new Map<string, FhirPackageProgress>();
	let totalPackages = 0;
	let completedPackages = 0;
	let totalResources = 0;
	let durationMs = 0;
	let isCompleted = false;

	for (const event of events) {
		switch (event.type) {
			case "started":
				totalPackages = event.total_packages;
				break;
			case "dependencies_resolved":
				for (const packageRef of event.packages) {
					const [name, version] = packageRef.split("@");
					if (name && version) {
						packageMap.set(packageRef, {
							name,
							version,
							status: "pending",
							downloadPercent: 0,
						});
					}
				}
				break;
			case "download_started":
				packageMap.set(`${event.package}@${event.version}`, {
					name: event.package,
					version: event.version,
					status: "downloading",
					downloadPercent: 0,
				});
				break;
			case "download_progress": {
				const progress = packageMap.get(`${event.package}@${event.version}`);
				if (progress) progress.downloadPercent = event.percent;
				break;
			}
			case "download_completed": {
				const progress = packageMap.get(`${event.package}@${event.version}`);
				if (progress) progress.downloadPercent = 100;
				break;
			}
			case "extracting": {
				const progress = packageMap.get(`${event.package}@${event.version}`);
				if (progress) progress.status = "extracting";
				break;
			}
			case "indexing": {
				const progress = packageMap.get(`${event.package}@${event.version}`);
				if (progress) progress.status = "indexing";
				break;
			}
			case "package_installed": {
				const progress = packageMap.get(`${event.package}@${event.version}`);
				if (progress) {
					progress.status = "completed";
					progress.resourceCount = event.resource_count;
				}
				completedPackages++;
				break;
			}
			case "skipped":
				packageMap.set(`${event.package}@${event.version}`, {
					name: event.package,
					version: event.version,
					status: "skipped",
					downloadPercent: 100,
					skipReason: event.reason,
				});
				completedPackages++;
				break;
			case "error":
				if (event.package && event.version) {
					const progress = packageMap.get(`${event.package}@${event.version}`);
					if (progress) {
						progress.status = "error";
						progress.errorMessage = event.message;
					}
				}
				break;
			case "completed":
				isCompleted = true;
				totalResources = event.total_resources;
				durationMs = event.duration_ms;
				break;
		}
	}

	return {
		packages: Array.from(packageMap.values()),
		totalPackages,
		completedPackages,
		overallProgress: totalPackages > 0 ? (completedPackages / totalPackages) * 100 : 0,
		isCompleted,
		completionStats: { totalResources, durationMs },
	};
}


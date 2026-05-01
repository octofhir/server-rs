import type {
	PackageInfo,
	PackageLookupResponse,
	PackageResourceSummary,
	PackageSearchResult,
	ResourceTypeSummary,
} from "@/shared/api/types";

export interface FhirVersionCompatibilityView {
	label: string;
	isCompatible: boolean;
	tooltip: string;
}

export interface FhirPackageInstalledView {
	id: string;
	name: string;
	versionLabel: string;
	rawVersion: string;
	rawFhirVersion?: string;
	fhirVersionLabel: string;
	resourceCountLabel: string;
	installedAtLabel: string;
	compatibility: FhirVersionCompatibilityView;
}

export interface FhirPackageResourceView {
	id: string;
	resourceType: string;
	nameLabel: string;
	urlLabel: string;
	versionLabel: string;
}

export interface FhirPackageRegistryView {
	id: string;
	name: string;
	descriptionLabel: string;
	latestVersionLabel: string;
}

export function normalizeFhirVersion(version?: string): string {
	if (!version) return "unknown";
	const value = version.toLowerCase().trim();

	if (value.startsWith("4.0")) return "R4";
	if (value.startsWith("4.3")) return "R4B";
	if (value.startsWith("5.0")) return "R5";
	if (value.startsWith("6.0")) return "R6";
	if (value === "r4" || value === "r4.0.1") return "R4";
	if (value === "r4b" || value === "r4.3.0") return "R4B";
	if (value === "r5" || value === "r5.0.0") return "R5";
	if (value === "r6") return "R6";

	return version.toUpperCase();
}

export function getFhirVersionCompatibilityView(
	packageVersion: string | undefined,
	serverVersion: string,
): FhirVersionCompatibilityView {
	const normalizedPackage = normalizeFhirVersion(packageVersion);
	const normalizedServer = normalizeFhirVersion(serverVersion);
	const isCompatible = !packageVersion || normalizedPackage === normalizedServer;

	return {
		label: packageVersion || "unknown",
		isCompatible,
		tooltip: isCompatible
			? `Compatible with server (${serverVersion})`
			: `Package is ${packageVersion}, server is ${serverVersion}`,
	};
}

export function filterFhirPackages(
	packages: PackageInfo[],
	search: string,
): PackageInfo[] {
	const query = search.trim().toLowerCase();
	if (!query) return packages;

	return packages.filter(
		(pkg) =>
			pkg.name.toLowerCase().includes(query) ||
			pkg.version.toLowerCase().includes(query),
	);
}

export function getFhirPackageInstalledViews(
	packages: PackageInfo[],
	serverVersion: string,
): FhirPackageInstalledView[] {
	return packages.map((pkg) => ({
		id: `${pkg.name}@${pkg.version}`,
		name: pkg.name,
		versionLabel: pkg.version,
		rawVersion: pkg.version,
		rawFhirVersion: pkg.fhirVersion,
		fhirVersionLabel: pkg.fhirVersion || "unknown",
		resourceCountLabel: String(pkg.resourceCount),
		installedAtLabel: pkg.installedAt
			? new Date(pkg.installedAt).toLocaleDateString()
			: "-",
		compatibility: getFhirVersionCompatibilityView(pkg.fhirVersion, serverVersion),
	}));
}

export function filterFhirPackageResources(
	resources: PackageResourceSummary[],
	search: string,
): PackageResourceSummary[] {
	const query = search.trim().toLowerCase();
	if (!query) return resources;

	return resources.filter(
		(resource) =>
			resource.name?.toLowerCase().includes(query) ||
			resource.url?.toLowerCase().includes(query) ||
			resource.id?.toLowerCase().includes(query),
	);
}

export function getFhirPackageResourceViews(
	resources: PackageResourceSummary[],
): FhirPackageResourceView[] {
	return resources.map((resource, index) => ({
		id: resource.url || resource.id || `${resource.resourceType}-${index}`,
		resourceType: resource.resourceType,
		nameLabel: resource.name || "-",
		urlLabel: resource.url || "-",
		versionLabel: resource.version || "-",
	}));
}

export function getFhirPackageResourceTypeOptions(
	resourceTypes: ResourceTypeSummary[],
): Array<{ value: string; label: string }> {
	return [
		{ value: "", label: "All types" },
		...resourceTypes.map((resourceType) => ({
			value: resourceType.resourceType,
			label: `${resourceType.resourceType} (${resourceType.count})`,
		})),
	];
}

export function getFhirPackageRegistryViews(
	packages: PackageSearchResult[],
): FhirPackageRegistryView[] {
	return packages.map((pkg) => ({
		id: pkg.name,
		name: pkg.name,
		descriptionLabel: pkg.description || "-",
		latestVersionLabel: pkg.latestVersion,
	}));
}

export function getFhirPackageVersionOptions(
	lookupData: PackageLookupResponse | undefined,
): Array<{ value: string; label: string; disabled?: boolean }> {
	if (!lookupData?.versions) return [];

	return lookupData.versions.map((version) => {
		const isInstalled = lookupData.installedVersions.includes(version);

		return {
			value: version,
			label: isInstalled ? `${version} (installed)` : version,
			disabled: isInstalled,
		};
	});
}

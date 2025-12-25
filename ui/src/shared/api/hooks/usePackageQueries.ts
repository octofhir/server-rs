import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useRef, useState } from "react";
import { serverApi } from "../serverApi";
import type { PackageInstallRequest } from "../types";

// Query keys for cache management
export const packageKeys = {
	all: ["packages"] as const,
	list: () => [...packageKeys.all, "list"] as const,
	detail: (name: string, version: string) => [...packageKeys.all, "detail", name, version] as const,
	resources: (name: string, version: string) => [...packageKeys.all, "resources", name, version] as const,
	resourcesFiltered: (
		name: string,
		version: string,
		params?: { resourceType?: string; limit?: number; offset?: number },
	) => [...packageKeys.resources(name, version), params] as const,
	resourceContent: (name: string, version: string, url: string) =>
		[...packageKeys.all, "resource", name, version, url] as const,
	fhirSchema: (name: string, version: string, url: string) =>
		[...packageKeys.all, "fhirschema", name, version, url] as const,
	lookup: (name: string) => [...packageKeys.all, "lookup", name] as const,
};

/**
 * Hook to fetch list of installed FHIR packages.
 */
export function usePackages() {
	return useQuery({
		queryKey: packageKeys.list(),
		queryFn: () => serverApi.getPackages(),
		staleTime: 1000 * 60 * 5, // 5 minutes
	});
}

/**
 * Hook to fetch details of a specific package.
 */
export function usePackageDetails(name: string, version: string) {
	return useQuery({
		queryKey: packageKeys.detail(name, version),
		queryFn: () => serverApi.getPackageDetails(name, version),
		enabled: !!name && !!version,
		staleTime: 1000 * 60 * 10, // 10 minutes
	});
}

/**
 * Hook to fetch resources in a package with optional filtering.
 */
export function usePackageResources(
	name: string,
	version: string,
	params?: { resourceType?: string; limit?: number; offset?: number },
) {
	return useQuery({
		queryKey: packageKeys.resourcesFiltered(name, version, params),
		queryFn: () => serverApi.getPackageResources(name, version, params),
		enabled: !!name && !!version,
		staleTime: 1000 * 60 * 10, // 10 minutes
	});
}

/**
 * Hook to fetch full content of a specific resource from a package.
 */
export function usePackageResourceContent(name: string, version: string, resourceUrl: string) {
	return useQuery({
		queryKey: packageKeys.resourceContent(name, version, resourceUrl),
		queryFn: () => serverApi.getPackageResourceContent(name, version, resourceUrl),
		enabled: !!name && !!version && !!resourceUrl,
		staleTime: 1000 * 60 * 30, // 30 minutes - resource content rarely changes
	});
}

/**
 * Hook to fetch FHIRSchema for a resource from a package.
 */
export function usePackageFhirSchema(name: string, version: string, resourceUrl: string) {
	return useQuery({
		queryKey: packageKeys.fhirSchema(name, version, resourceUrl),
		queryFn: () => serverApi.getPackageFhirSchema(name, version, resourceUrl),
		enabled: !!name && !!version && !!resourceUrl,
		staleTime: 1000 * 60 * 60, // 1 hour - schemas are stable
	});
}

/**
 * Hook to lookup available versions for a package from the FHIR registry.
 */
export function usePackageLookup(name: string) {
	return useQuery({
		queryKey: packageKeys.lookup(name),
		queryFn: () => serverApi.lookupPackage(name),
		enabled: !!name && name.length >= 3, // Only search when name has at least 3 chars
		staleTime: 1000 * 60 * 10, // 10 minutes
		retry: false, // Don't retry on 404 (package not found)
	});
}

/**
 * Hook to search for packages in the FHIR registry.
 * Supports partial matching (ILIKE) - spaces in query are treated as wildcards.
 */
export function usePackageSearch(query: string) {
	return useQuery({
		queryKey: [...packageKeys.all, "search", query] as const,
		queryFn: () => serverApi.searchPackages(query),
		enabled: !!query && query.length >= 2, // Only search when query has at least 2 chars
		staleTime: 1000 * 60 * 5, // 5 minutes
	});
}

/**
 * Hook to install a package from the FHIR registry.
 * Invalidates package list cache on success.
 */
export function useInstallPackage() {
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: (request: PackageInstallRequest) => serverApi.installPackage(request),
		onSuccess: (data) => {
			// Invalidate package list to refresh installed packages
			queryClient.invalidateQueries({ queryKey: packageKeys.list() });
			// Invalidate lookup for this package to update installed versions
			queryClient.invalidateQueries({ queryKey: packageKeys.lookup(data.name) });
		},
	});
}

/**
 * Hook to install a package with SSE progress streaming.
 * This provides real-time progress updates during installation.
 *
 * @example
 * const { install, abort, events, isInstalling, error } = useInstallPackageWithProgress();
 *
 * // Start installation
 * install({ name: "hl7.fhir.us.core", version: "6.1.0" });
 *
 * // Track progress
 * useEffect(() => {
 *   const latest = events[events.length - 1];
 *   if (latest?.type === "download_progress") {
 *     console.log(`Downloading: ${latest.percent}%`);
 *   }
 * }, [events]);
 */
export function useInstallPackageWithProgress() {
	const queryClient = useQueryClient();
	const [events, setEvents] = useState<import("../types").InstallEvent[]>([]);
	const [isInstalling, setIsInstalling] = useState(false);
	const [error, setError] = useState<Error | null>(null);
	const abortRef = useRef<(() => void) | null>(null);

	const install = useCallback(
		(request: PackageInstallRequest) => {
			setEvents([]);
			setError(null);
			setIsInstalling(true);

			abortRef.current = serverApi.installPackageWithProgress(
				request,
				(event) => {
					setEvents((prev) => [...prev, event]);
				},
				(err) => {
					setError(err);
					setIsInstalling(false);
				},
				() => {
					setIsInstalling(false);
					// Invalidate caches on completion
					queryClient.invalidateQueries({ queryKey: packageKeys.list() });
					queryClient.invalidateQueries({ queryKey: packageKeys.lookup(request.name) });
				},
			);
		},
		[queryClient],
	);

	const abort = useCallback(() => {
		abortRef.current?.();
		setIsInstalling(false);
	}, []);

	const reset = useCallback(() => {
		setEvents([]);
		setError(null);
		setIsInstalling(false);
	}, []);

	return {
		install,
		abort,
		reset,
		events,
		isInstalling,
		error,
		latestEvent: events[events.length - 1] ?? null,
	};
}

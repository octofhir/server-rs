import type { FhirBundle, FhirResource } from "@/shared/api/types";

export interface BundlePaginationInfo {
  total: number;
  hasNext: boolean;
  hasPrev: boolean;
  hasFirst: boolean;
  hasLast: boolean;
  nextUrl?: string;
  prevUrl?: string;
  firstUrl?: string;
  lastUrl?: string;
}

/**
 * Extract pagination information from a FHIR Bundle
 */
export const getBundlePaginationInfo = (bundle: FhirBundle): BundlePaginationInfo => {
  const links = bundle.link || [];
  const total = bundle.total || 0;

  const linkMap = links.reduce(
    (acc, link) => {
      if (link.relation && link.url) {
        acc[link.relation] = link.url;
      }
      return acc;
    },
    {} as Record<string, string>
  );

  return {
    total,
    hasNext: Boolean(linkMap.next),
    hasPrev: Boolean(linkMap.prev || linkMap.previous),
    hasFirst: Boolean(linkMap.first),
    hasLast: Boolean(linkMap.last),
    nextUrl: linkMap.next,
    prevUrl: linkMap.prev || linkMap.previous,
    firstUrl: linkMap.first,
    lastUrl: linkMap.last,
  };
};

/**
 * Calculate current page number from bundle links and count
 */
export const getCurrentPageFromBundle = (bundle: FhirBundle, pageSize: number): number => {
  const links = bundle.link || [];
  const selfLink = links.find((link) => link.relation === "self");

  if (!selfLink?.url) {
    return 1;
  }

  try {
    const url = new URL(selfLink.url);
    const offset = Number(url.searchParams.get("_offset")) || 0;
    return Math.floor(offset / pageSize) + 1;
  } catch {
    return 1;
  }
};

/**
 * Extract search parameters from a Bundle self link
 */
export const getSearchParamsFromBundle = (bundle: FhirBundle): Record<string, string> => {
  const links = bundle.link || [];
  const selfLink = links.find((link) => link.relation === "self");

  if (!selfLink?.url) {
    return {};
  }

  try {
    const url = new URL(selfLink.url);
    const params: Record<string, string> = {};

    url.searchParams.forEach((value, key) => {
      params[key] = value;
    });

    return params;
  } catch {
    return {};
  }
};

/**
 * Check if bundle has any resources
 */
export const bundleHasResources = (bundle: FhirBundle): boolean => {
  return Boolean(bundle.entry && bundle.entry.length > 0);
};

/**
 * Get resource count from bundle
 */
export const getBundleResourceCount = (bundle: FhirBundle): number => {
  return bundle.entry?.length || 0;
};

/**
 * Extract resources from bundle safely
 */
export const getResourcesFromBundle = <T extends FhirResource = FhirResource>(
  bundle: FhirBundle,
): T[] => {
  if (!bundle.entry) {
    return [];
  }
  return bundle.entry.map((entry) => entry.resource).filter(Boolean) as T[];
};

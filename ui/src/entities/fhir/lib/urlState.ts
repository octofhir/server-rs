import { useUnit } from "effector-react";
import { useEffect } from "react";
import { useUrlState } from "@/shared/lib/url-state";
import {
  $pagination,
  $searchParams,
  $selectedResourceType,
  setSearchParams,
  setSelectedResourceType,
} from "../model/resourceBrowser";

// URL state configurations
export const resourceTypeConfig = {
  serialize: (value: string | null): string => value || "",
  deserialize: (value: string): string | null => value || null,
  defaultValue: null as string | null,
};

export const searchParamsConfig = {
  serialize: (params: Record<string, string>): string => {
    const filteredParams = Object.fromEntries(
      Object.entries(params).filter(([, value]) => value !== "")
    );
    return JSON.stringify(filteredParams);
  },
  deserialize: (value: string): Record<string, string> => {
    try {
      const parsed = JSON.parse(value);
      return typeof parsed === "object" && parsed !== null ? parsed : {};
    } catch {
      return {};
    }
  },
  defaultValue: {} as Record<string, string>,
};

export const paginationConfig = {
  serialize: (page: number): string => page.toString(),
  deserialize: (value: string): number => {
    const parsed = parseInt(value, 10);
    return Number.isNaN(parsed) || parsed < 1 ? 1 : parsed;
  },
  defaultValue: 1,
};

// Custom hooks for FHIR URL state management
export function useResourceBrowserUrlState() {
  // URL state hooks that handle sync automatically
  const [urlResourceType, setUrlResourceType] = useUrlState("resourceType", resourceTypeConfig);
  const [urlSearchParams, setUrlSearchParams] = useUrlState("search", searchParamsConfig);
  const [urlPage, setUrlPage] = useUrlState("page", paginationConfig);

  // Effector state
  const selectedResourceType = useUnit($selectedResourceType);
  const searchParams = useUnit($searchParams);
  const pagination = useUnit($pagination);

  // Only sync from URL to Effector store on initial load or URL changes
  useEffect(() => {
    setSelectedResourceType(urlResourceType);
  }, [urlResourceType]);

  useEffect(() => {
    if (Object.keys(urlSearchParams).length > 0) {
      setSearchParams(urlSearchParams);
    }
  }, [urlSearchParams]);

  // Only sync from Effector store to URL when store changes
  useEffect(() => {
    setUrlResourceType(selectedResourceType);
  }, [selectedResourceType]);

  useEffect(() => {
    setUrlSearchParams(searchParams);
  }, [searchParams]);

  useEffect(() => {
    setUrlPage(pagination.currentPage);
  }, [pagination.currentPage]);

  // Return current URL state
  return {
    urlResourceType,
    urlSearchParams,
    urlPage,
  };
}

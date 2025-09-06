import { createEffect, createEvent, createStore, sample } from "effector";
import { fhirClient } from "@/shared/api";
import type { FhirBundle, FhirResource } from "@/shared/api/types";

// Types
export interface ResourceBrowserState {
  selectedResourceType: string | null;
  resourceList: {
    data: FhirResource[];
    total: number;
    loading: boolean;
    error: string | null;
  };
  selectedResource: FhirResource | null;
  searchParams: Record<string, string>;
  pagination: {
    count: number;
    currentPage: number;
    links: {
      first?: string;
      prev?: string;
      next?: string;
      last?: string;
    };
  };
}

export interface SearchResourcesParams {
  resourceType: string;
  params: Record<string, string>;
}

export interface FetchResourceParams {
  resourceType: string;
  resourceId: string;
}

export interface DeleteResourceParams {
  resourceType: string;
  resourceId: string;
}

// Events
export const setSelectedResourceType = createEvent<string | null>();
export const setSelectedResource = createEvent<FhirResource | null>();
export const setSearchParams = createEvent<Record<string, string>>();
export const setPageCount = createEvent<number>();
export const clearResourceList = createEvent();
export const clearSelectedResource = createEvent();

// Effects
export const searchResourcesFx = createEffect<SearchResourcesParams, FhirBundle>();
export const fetchResourceFx = createEffect<FetchResourceParams, FhirResource>();
export const deleteResourceFx = createEffect<DeleteResourceParams, void>();
export const navigateToPageFx = createEffect<string, FhirBundle>();

// Store
const initialState: ResourceBrowserState = {
  selectedResourceType: null,
  resourceList: {
    data: [],
    total: 0,
    loading: false,
    error: null,
  },
  selectedResource: null,
  searchParams: { _count: "20" },
  pagination: {
    count: 20,
    currentPage: 1,
    links: {},
  },
};

export const $resourceBrowser = createStore<ResourceBrowserState>(initialState);

// Derived stores
export const $selectedResourceType = $resourceBrowser.map((state) => state.selectedResourceType);
export const $resourceList = $resourceBrowser.map((state) => state.resourceList);
export const $selectedResource = $resourceBrowser.map((state) => state.selectedResource);
export const $searchParams = $resourceBrowser.map((state) => state.searchParams);
export const $pagination = $resourceBrowser.map((state) => state.pagination);

// Effects implementations
searchResourcesFx.use(async ({ resourceType, params }) => {
  const bundle = await fhirClient.search<FhirResource>(resourceType, params);
  return bundle;
});

fetchResourceFx.use(async ({ resourceType, resourceId }) => {
  const resource = await fhirClient.read<FhirResource>(resourceType, resourceId);
  return resource;
});

deleteResourceFx.use(async ({ resourceType, resourceId }) => {
  await fhirClient.delete(resourceType, resourceId);
});

navigateToPageFx.use(async (url) => {
  const response = await fhirClient.customRequest<FhirBundle>({
    method: "GET",
    url,
  });
  return response.data;
});

// Store updates
$resourceBrowser
  .on(setSelectedResourceType, (state, resourceType) => ({
    ...state,
    selectedResourceType: resourceType,
    selectedResource: null,
    resourceList: { ...state.resourceList, data: [], total: 0, error: null },
    pagination: { ...state.pagination, currentPage: 1, links: {} },
  }))
  .on(setSelectedResource, (state, resource) => ({
    ...state,
    selectedResource: resource,
  }))
  .on(setSearchParams, (state, params) => ({
    ...state,
    searchParams: { ...state.searchParams, ...params },
  }))
  .on(setPageCount, (state, count) => ({
    ...state,
    searchParams: { ...state.searchParams, _count: String(count) },
    pagination: { ...state.pagination, count },
  }))
  .on(clearResourceList, (state) => ({
    ...state,
    resourceList: { ...state.resourceList, data: [], total: 0, error: null },
    pagination: { ...state.pagination, currentPage: 1, links: {} },
  }))
  .on(clearSelectedResource, (state) => ({
    ...state,
    selectedResource: null,
  }));

// Loading states
$resourceBrowser
  .on(searchResourcesFx.pending, (state, loading) => ({
    ...state,
    resourceList: { ...state.resourceList, loading, error: null },
  }))
  .on(navigateToPageFx.pending, (state, loading) => ({
    ...state,
    resourceList: { ...state.resourceList, loading, error: null },
  }));

// Success handling
$resourceBrowser
  .on(searchResourcesFx.doneData, (state, bundle) => {
    const resources =
      (bundle.entry?.map((entry) => entry.resource).filter(Boolean) as FhirResource[]) || [];
    const total = bundle.total || 0;

    const links: ResourceBrowserState["pagination"]["links"] = {};
    bundle.link?.forEach((link) => {
      if (link.relation && link.url) {
        links[link.relation as keyof typeof links] = link.url;
      }
    });

    return {
      ...state,
      resourceList: {
        data: resources,
        total,
        loading: false,
        error: null,
      },
      pagination: {
        ...state.pagination,
        links,
      },
    };
  })
  .on(navigateToPageFx.doneData, (state, bundle) => {
    const resources =
      (bundle.entry?.map((entry) => entry.resource).filter(Boolean) as FhirResource[]) || [];
    const total = bundle.total || 0;

    const links: ResourceBrowserState["pagination"]["links"] = {};
    bundle.link?.forEach((link) => {
      if (link.relation && link.url) {
        links[link.relation as keyof typeof links] = link.url;
      }
    });

    return {
      ...state,
      resourceList: {
        data: resources,
        total,
        loading: false,
        error: null,
      },
      pagination: {
        ...state.pagination,
        links,
      },
    };
  })
  .on(fetchResourceFx.doneData, (state, resource) => ({
    ...state,
    selectedResource: resource,
  }));

// Error handling
$resourceBrowser
  .on(searchResourcesFx.failData, (state, error) => ({
    ...state,
    resourceList: {
      ...state.resourceList,
      loading: false,
      error: error instanceof Error ? error.message : "Failed to search resources",
    },
  }))
  .on(navigateToPageFx.failData, (state, error) => ({
    ...state,
    resourceList: {
      ...state.resourceList,
      loading: false,
      error: error instanceof Error ? error.message : "Failed to navigate to page",
    },
  }))
  .on(fetchResourceFx.failData, (state, error) => ({
    ...state,
    selectedResource: null,
    resourceList: {
      ...state.resourceList,
      error: error instanceof Error ? error.message : "Failed to fetch resource",
    },
  }));

// Trigger search when resource type or params change
sample({
  source: { resourceType: $selectedResourceType, params: $searchParams },
  filter: ({ resourceType }) => resourceType !== null,
  fn: ({ resourceType, params }) => ({
    resourceType: resourceType!,
    params,
  }),
  target: searchResourcesFx,
});

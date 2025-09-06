import { createEffect, createEvent, createStore, sample } from "effector";
import type { FhirBundle, FhirResource } from "@/shared/api";
import { fhirClient } from "@/shared/api";

// Events
export const loadResource = createEvent<{ resourceType: string; id: string }>();
export const searchResources = createEvent<{
  resourceType: string;
  params?: Record<string, string | number>;
}>();
export const createResource = createEvent<FhirResource>();
export const updateResource = createEvent<{
  resourceType: string;
  id: string;
  resource: FhirResource;
}>();
export const deleteResource = createEvent<{ resourceType: string; id: string }>();
export const resetFhirState = createEvent();
export const setSelectedResource = createEvent<FhirResource | null>();
export const setActiveResourceType = createEvent<string | null>();

// Effects
export const loadResourceFx = createEffect<{ resourceType: string; id: string }, FhirResource>();
export const searchResourcesFx = createEffect<
  { resourceType: string; params?: Record<string, string | number> },
  FhirBundle
>();
export const createResourceFx = createEffect<FhirResource, FhirResource>();
export const updateResourceFx = createEffect<
  { resourceType: string; id: string; resource: FhirResource },
  FhirResource
>();
export const deleteResourceFx = createEffect<{ resourceType: string; id: string }, void>();

// Stores
export const $currentResource = createStore<FhirResource | null>(null);
export const $searchResults = createStore<FhirBundle | null>(null);
export const $selectedResource = createStore<FhirResource | null>(null);
export const $activeResourceType = createStore<string | null>(null);
export const $resourceCache = createStore<Record<string, FhirResource>>({});
export const $searchCache = createStore<Record<string, FhirBundle>>({});
export const $fhirLoading = createStore(false);
export const $fhirError = createStore<string | null>(null);

// Effects implementations
loadResourceFx.use(async ({ resourceType, id }) => {
  return await fhirClient.read(resourceType, id);
});

searchResourcesFx.use(async ({ resourceType, params }) => {
  return await fhirClient.search(resourceType, params);
});

createResourceFx.use(async (resource) => {
  return await fhirClient.create(resource);
});

updateResourceFx.use(async ({ resource }) => {
  return await fhirClient.update(resource);
});

deleteResourceFx.use(async ({ resourceType, id }) => {
  await fhirClient.delete(resourceType, id);
});

// Store updates
$currentResource.on(loadResourceFx.doneData, (_, resource) => resource);
$searchResults.on(searchResourcesFx.doneData, (_, results) => results);
$selectedResource.on(setSelectedResource, (_, resource) => resource);
$activeResourceType.on(setActiveResourceType, (_, type) => type);

// Cache management
$resourceCache.on(loadResourceFx.doneData, (cache, resource) => ({
  ...cache,
  [`${resource.resourceType}/${resource.id}`]: resource,
}));

$resourceCache.on(createResourceFx.doneData, (cache, resource) => ({
  ...cache,
  [`${resource.resourceType}/${resource.id}`]: resource,
}));

$resourceCache.on(updateResourceFx.doneData, (cache, resource) => ({
  ...cache,
  [`${resource.resourceType}/${resource.id}`]: resource,
}));

// Loading states
$fhirLoading
  .on(loadResourceFx.pending, (_, pending) => pending)
  .on(searchResourcesFx.pending, (_, pending) => pending)
  .on(createResourceFx.pending, (_, pending) => pending)
  .on(updateResourceFx.pending, (_, pending) => pending)
  .on(deleteResourceFx.pending, (_, pending) => pending);

// Error handling
$fhirError
  .on(loadResourceFx.failData, (_, error) => error?.message || "Failed to load resource")
  .on(searchResourcesFx.failData, (_, error) => error?.message || "Search failed")
  .on(createResourceFx.failData, (_, error) => error?.message || "Failed to create resource")
  .on(updateResourceFx.failData, (_, error) => error?.message || "Failed to update resource")
  .on(deleteResourceFx.failData, (_, error) => error?.message || "Failed to delete resource")
  .reset(loadResourceFx, searchResourcesFx, createResourceFx, updateResourceFx, deleteResourceFx);

// Reset state
sample({
  clock: resetFhirState,
  target: [
    $currentResource.reinit!,
    $searchResults.reinit!,
    $selectedResource.reinit!,
    $resourceCache.reinit!,
    $searchCache.reinit!,
    $fhirError.reinit!,
  ],
});

// Wire events to effects
sample({
  clock: loadResource,
  target: loadResourceFx,
});

sample({
  clock: searchResources,
  target: searchResourcesFx,
});

sample({
  clock: createResource,
  target: createResourceFx,
});

sample({
  clock: updateResource,
  target: updateResourceFx,
});

sample({
  clock: deleteResource,
  target: deleteResourceFx,
});

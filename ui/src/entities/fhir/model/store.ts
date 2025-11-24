import { createSignal, createResource } from "solid-js";
import { fhirClient } from "@/shared/api";
import type { FhirBundle, FhirResource, CapabilityStatement } from "@/shared/api";

// Capabilities
const [capabilities, setCapabilities] = createSignal<CapabilityStatement | null>(null);
const [capabilitiesLoading, setCapabilitiesLoading] = createSignal(false);
const [capabilitiesError, setCapabilitiesError] = createSignal<string | null>(null);

export const loadCapabilities = async () => {
  setCapabilitiesLoading(true);
  setCapabilitiesError(null);
  try {
    const data = await fhirClient.getCapabilities();
    setCapabilities(data);
    return data;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to load capabilities";
    setCapabilitiesError(message);
    throw err;
  } finally {
    setCapabilitiesLoading(false);
  }
};

export const getResourceTypes = () => {
  const caps = capabilities();
  if (!caps?.rest?.[0]?.resource) return [];
  return caps.rest[0].resource.map((r) => r.type).sort();
};

// Resource Browser State
const [selectedResourceType, setSelectedResourceType] = createSignal<string | null>(null);
const [selectedResource, setSelectedResource] = createSignal<FhirResource | null>(null);
const [resources, setResources] = createSignal<FhirResource[]>([]);
const [bundle, setBundle] = createSignal<FhirBundle | null>(null);
const [resourcesLoading, setResourcesLoading] = createSignal(false);
const [resourcesError, setResourcesError] = createSignal<string | null>(null);

export const searchResources = async (
  resourceType: string,
  params: Record<string, string> = {},
) => {
  setResourcesLoading(true);
  setResourcesError(null);
  setSelectedResourceType(resourceType);
  try {
    const result = await fhirClient.search(resourceType, params);
    setBundle(result);
    setResources(result.entry?.map((e) => e.resource!).filter(Boolean) || []);
    return result;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to search resources";
    setResourcesError(message);
    throw err;
  } finally {
    setResourcesLoading(false);
  }
};

export const loadResource = async (resourceType: string, id: string) => {
  setResourcesLoading(true);
  setResourcesError(null);
  try {
    const result = await fhirClient.read(resourceType, id);
    setSelectedResource(result);
    return result;
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to load resource";
    setResourcesError(message);
    throw err;
  } finally {
    setResourcesLoading(false);
  }
};

export const clearSelectedResource = () => {
  setSelectedResource(null);
};

// Exports
export {
  capabilities,
  capabilitiesLoading,
  capabilitiesError,
  selectedResourceType,
  setSelectedResourceType,
  selectedResource,
  setSelectedResource,
  resources,
  bundle,
  resourcesLoading,
  resourcesError,
};

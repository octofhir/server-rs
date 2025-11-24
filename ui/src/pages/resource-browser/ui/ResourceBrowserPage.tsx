import { createSignal, onMount, For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { Card, Loader, JsonViewer, Splitter } from "@/shared/ui";
import {
  loadCapabilities,
  getResourceTypes,
  searchResources,
  loadResource,
  resources,
  selectedResource,
  resourcesLoading,
  bundle,
  clearSelectedResource,
} from "@/entities/fhir";
import styles from "./ResourceBrowserPage.module.css";

export const ResourceBrowserPage = () => {
  const params = useParams();
  const [resourceTypes, setResourceTypes] = createSignal<string[]>([]);
  const [selectedType, setSelectedType] = createSignal<string | null>(null);

  onMount(async () => {
    try {
      await loadCapabilities();
      setResourceTypes(getResourceTypes());

      if (params.type) {
        setSelectedType(params.type);
        await searchResources(params.type);
      }
    } catch (err) {
      console.error("Failed to load:", err);
    }
  });

  const handleTypeSelect = async (type: string) => {
    setSelectedType(type);
    clearSelectedResource();
    await searchResources(type);
  };

  const handleResourceSelect = async (resourceType: string, id: string) => {
    await loadResource(resourceType, id);
  };

  return (
    <div class={styles.container}>
      <Splitter direction="horizontal" defaultSize={20} minSize={15} maxSize={35}>
        <div class={styles.typesPanel}>
          <h3 class={styles.panelTitle}>Resource Types</h3>
          <div class={styles.typesList}>
            <For each={resourceTypes()}>
              {(type) => (
                <button
                  type="button"
                  class={styles.typeItem}
                  classList={{ [styles.active]: selectedType() === type }}
                  onClick={() => handleTypeSelect(type)}
                >
                  {type}
                </button>
              )}
            </For>
          </div>
        </div>

        <Splitter direction="horizontal" defaultSize={50} minSize={30} maxSize={70}>
          <div class={styles.listPanel}>
            <h3 class={styles.panelTitle}>
              {selectedType() || "Resources"}
              <Show when={bundle()}>
                <span class={styles.count}>({bundle()?.total || 0})</span>
              </Show>
            </h3>

            <Show when={resourcesLoading()}>
              <div class={styles.loaderContainer}>
                <Loader label="Loading..." />
              </div>
            </Show>

            <Show when={!resourcesLoading()}>
              <div class={styles.resourceList}>
                <Show
                  when={resources().length > 0}
                  fallback={
                    <div class={styles.emptyState}>
                      {selectedType() ? "No resources found" : "Select a resource type"}
                    </div>
                  }
                >
                  <For each={resources()}>
                    {(resource) => (
                      <Card
                        class={styles.resourceCard}
                        hoverable
                        onClick={() => handleResourceSelect(resource.resourceType, resource.id!)}
                      >
                        <div class={styles.resourceId}>{resource.id}</div>
                        <div class={styles.resourceMeta}>
                          {resource.meta?.lastUpdated
                            ? new Date(resource.meta.lastUpdated).toLocaleString()
                            : "No date"}
                        </div>
                      </Card>
                    )}
                  </For>
                </Show>
              </div>
            </Show>
          </div>

          <div class={styles.detailsPanel}>
            <h3 class={styles.panelTitle}>Details</h3>
            <Show
              when={selectedResource()}
              fallback={<div class={styles.emptyState}>Select a resource to view details</div>}
            >
              <JsonViewer data={selectedResource()} />
            </Show>
          </div>
        </Splitter>
      </Splitter>
    </div>
  );
};

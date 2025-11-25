import {
  createSignal,
  createMemo,
  createEffect,
  onMount,
  For,
  Show,
  type Component,
} from "solid-js";
import { useUnit } from "effector-solid";
import { $selectedResourceType, setSelectedResourceType } from "@/entities/fhir";
import { serverApi } from "@/shared/api";
import { Button, Input } from "@/shared/ui";
import { IconSearch, IconRefresh, IconLoader } from "@/shared/ui/Icon";
import styles from "./ResourceTypeList.module.css";

interface ResourceTypeListProps {
  class?: string;
  onResourceTypeSelect?: () => void;
}

interface ResourceType {
  name: string;
  count?: number;
}

// Simple debounce hook
const createDebouncedValue = <T,>(value: () => T, delay: number) => {
  const [debouncedValue, setDebouncedValue] = createSignal<T>(value());

  createEffect(() => {
    const v = value();
    const timer = setTimeout(() => setDebouncedValue(() => v), delay);
    return () => clearTimeout(timer);
  });

  return debouncedValue;
};

export const ResourceTypeList: Component<ResourceTypeListProps> = (props) => {
  const [resourceTypes, setResourceTypes] = createSignal<ResourceType[]>([]);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [searchTerm, setSearchTerm] = createSignal("");

  const debouncedSearch = createDebouncedValue(searchTerm, 300);
  const selectedResourceType = useUnit($selectedResourceType);

  // Load resource types from server
  const loadResourceTypes = async () => {
    setLoading(true);
    setError(null);

    try {
      const types = await serverApi.getResourceTypes();
      const resourceTypeObjects = types.map((name) => ({ name }));
      setResourceTypes(resourceTypeObjects);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load resource types");
    } finally {
      setLoading(false);
    }
  };

  // Initial load
  onMount(() => {
    loadResourceTypes();
  });

  // Filter resource types based on search
  const filteredResourceTypes = createMemo(() =>
    resourceTypes().filter((type) =>
      type.name.toLowerCase().includes(debouncedSearch().toLowerCase())
    )
  );

  // Group resource types alphabetically
  const groupedResourceTypes = createMemo(() => {
    const groups = new Map<string, ResourceType[]>();

    for (const type of filteredResourceTypes()) {
      const firstLetter = type.name[0].toUpperCase();
      const existing = groups.get(firstLetter) || [];
      existing.push(type);
      groups.set(firstLetter, existing);
    }

    return Array.from(groups.entries())
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([letter, types]) => ({ letter, types }));
  });

  const handleResourceTypeSelect = (resourceType: string) => {
    setSelectedResourceType(resourceType);
    props.onResourceTypeSelect?.();
  };

  // Loading state
  if (loading() && resourceTypes().length === 0) {
    return (
      <div class={`${styles.container} ${props.class || ""}`}>
        <div class={styles.header}>
          <span class={styles.headerTitle}>Resource Types</span>
        </div>
        <div class={styles.loadingContainer}>
          <IconLoader size={20} />
          <span class={styles.loadingText}>Loading resource types...</span>
        </div>
      </div>
    );
  }

  // Error state (no data)
  if (error() && resourceTypes().length === 0) {
    return (
      <div class={`${styles.container} ${props.class || ""}`}>
        <div class={styles.header}>
          <span class={styles.headerTitle}>Resource Types</span>
          <button class={styles.iconButton} onClick={loadResourceTypes} title="Refresh">
            <IconRefresh size={14} />
          </button>
        </div>
        <div class={styles.errorContainer}>
          <span class={styles.errorText}>{error()}</span>
          <Button size="sm" variant="outline" onClick={loadResourceTypes}>
            Retry
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div class={`${styles.container} ${props.class || ""}`}>
      <div class={styles.header}>
        <span class={styles.headerTitle}>Resource Types</span>
        <div class={styles.headerActions}>
          <Show when={loading()}>
            <IconLoader size={14} />
          </Show>
          <button
            class={styles.iconButton}
            onClick={loadResourceTypes}
            disabled={loading()}
            title="Refresh"
          >
            <IconRefresh size={14} />
          </button>
        </div>
      </div>

      <div class={styles.searchContainer}>
        <Input
          placeholder="Search resource types..."
          value={searchTerm()}
          onInput={(e) => setSearchTerm(e.currentTarget.value)}
          icon={<IconSearch size={16} />}
        />
      </div>

      <div class={styles.listContainer}>
        <Show
          when={filteredResourceTypes().length > 0}
          fallback={
            <div class={styles.emptyState}>
              <span class={styles.emptyText}>
                {debouncedSearch() ? "No matching resource types" : "No resource types available"}
              </span>
            </div>
          }
        >
          <For each={groupedResourceTypes()}>
            {(group) => (
              <div class={styles.group}>
                <div class={styles.groupHeader}>{group.letter}</div>
                <For each={group.types}>
                  {(type) => (
                    <div
                      class={`${styles.resourceType} ${
                        selectedResourceType() === type.name ? styles.selected : ""
                      }`}
                      onClick={() => handleResourceTypeSelect(type.name)}
                      tabIndex={0}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          handleResourceTypeSelect(type.name);
                        }
                      }}
                    >
                      <span class={styles.resourceTypeName}>{type.name}</span>
                      <Show when={type.count !== undefined}>
                        <span class={styles.badge}>{type.count}</span>
                      </Show>
                    </div>
                  )}
                </For>
              </div>
            )}
          </For>
        </Show>
      </div>

      <Show when={error() && resourceTypes().length > 0}>
        <div class={styles.errorBanner}>
          <span class={styles.errorBannerText}>Failed to refresh: {error()}</span>
        </div>
      </Show>
    </div>
  );
};

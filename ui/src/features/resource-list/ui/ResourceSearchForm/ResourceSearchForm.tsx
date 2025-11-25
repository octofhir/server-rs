import { type Component, Show, For, createSignal, createEffect, createMemo } from "solid-js";
import { useUnit } from "effector-solid";
import {
  $searchParams,
  $selectedResourceType,
  setSearchParams,
} from "@/entities/fhir";
import { Input, Select, Button } from "@/shared/ui";
import { IconSearch, IconFilter, IconX, IconPlus } from "@/shared/ui/Icon";
import styles from "./ResourceSearchForm.module.css";

interface ResourceSearchFormProps {
  class?: string;
}

// Debounce helper
function createDebouncedValue<T>(value: () => T, delay: number): () => T {
  const [debouncedValue, setDebouncedValue] = createSignal<T>(value());

  createEffect(() => {
    const currentValue = value();
    const timeoutId = setTimeout(() => {
      setDebouncedValue(() => currentValue);
    }, delay);
    return () => clearTimeout(timeoutId);
  });

  return debouncedValue;
}

export const ResourceSearchForm: Component<ResourceSearchFormProps> = (props) => {
  const searchParams = useUnit($searchParams);
  const selectedResourceType = useUnit($selectedResourceType);

  // Local form state
  const [textSearch, setTextSearch] = createSignal(searchParams()._text || "");
  const [contentSearch, setContentSearch] = createSignal(searchParams()._content || "");
  const [status, setStatus] = createSignal(searchParams().status || "");
  const [active, setActive] = createSignal(searchParams().active || "");
  const [customParam, setCustomParam] = createSignal("");
  const [customValue, setCustomValue] = createSignal("");
  const [showAdvanced, setShowAdvanced] = createSignal(false);

  // Debounced search values
  const debouncedText = createDebouncedValue(textSearch, 500);
  const debouncedContent = createDebouncedValue(contentSearch, 500);

  // Update search params when debounced text changes
  createEffect(() => {
    const text = debouncedText();
    if (text) {
      setSearchParams({ _text: text, _count: "20" });
    }
  });

  // Update search params when debounced content changes
  createEffect(() => {
    const content = debouncedContent();
    if (content) {
      setSearchParams({ _content: content, _count: "20" });
    }
  });

  const handleFilterChange = (field: string, value: string | null) => {
    if (value && value !== "") {
      setSearchParams({ [field]: value, _count: "20" });
    } else {
      setSearchParams({ _count: "20" });
    }

    // Update local state
    if (field === "status") setStatus(value || "");
    if (field === "active") setActive(value || "");
  };

  const handleCustomParameterAdd = () => {
    const param = customParam();
    const val = customValue();

    if (param && val) {
      setSearchParams({ [param]: val, _count: "20" });
      setCustomParam("");
      setCustomValue("");
    }
  };

  const handleClearAll = () => {
    setSearchParams({ _count: "20" });
    setTextSearch("");
    setContentSearch("");
    setStatus("");
    setActive("");
    setCustomParam("");
    setCustomValue("");
  };

  const hasActiveFilters = createMemo(() => {
    const params = searchParams();
    return Object.keys(params).some(
      (key) => key !== "_count" && params[key]
    );
  });

  // Status options based on resource type
  const getStatusOptions = createMemo(() => {
    const resourceType = selectedResourceType();
    const commonOptions = [
      { value: "", label: "Any status" },
      { value: "active", label: "Active" },
      { value: "inactive", label: "Inactive" },
    ];

    switch (resourceType) {
      case "Patient":
        return [
          { value: "", label: "Any status" },
          { value: "active", label: "Active" },
          { value: "inactive", label: "Inactive" },
          { value: "deceased", label: "Deceased" },
        ];
      case "Observation":
        return [
          { value: "", label: "Any status" },
          { value: "final", label: "Final" },
          { value: "preliminary", label: "Preliminary" },
          { value: "cancelled", label: "Cancelled" },
          { value: "entered-in-error", label: "Error" },
        ];
      default:
        return commonOptions;
    }
  });

  const activeFilterEntries = createMemo(() => {
    const params = searchParams();
    return Object.entries(params).filter(
      ([key, value]) => key !== "_count" && value
    );
  });

  return (
    <div class={`${styles.container} ${props.class || ""}`}>
      <div class={styles.header}>
        <span class={styles.title}>
          Search {selectedResourceType() || "Resources"}
        </span>
        <div class={styles.headerActions}>
          <button
            class={`${styles.iconButton} ${showAdvanced() ? styles.active : ""}`}
            onClick={() => setShowAdvanced(!showAdvanced())}
            title="Advanced filters"
          >
            <IconFilter size={14} />
          </button>
          <Show when={hasActiveFilters()}>
            <button
              class={`${styles.iconButton} ${styles.danger}`}
              onClick={handleClearAll}
              title="Clear all filters"
            >
              <IconX size={14} />
            </button>
          </Show>
        </div>
      </div>

      <div class={styles.searchForm}>
        <Input
          placeholder="Search in text fields..."
          icon={<IconSearch size={16} />}
          value={textSearch()}
          onInput={(e) => setTextSearch(e.currentTarget.value)}
          fullWidth
        />

        <Show when={showAdvanced()}>
          <div class={styles.advancedFilters}>
            <Input
              label="Content Search"
              placeholder="Search in all content..."
              value={contentSearch()}
              onInput={(e) => setContentSearch(e.currentTarget.value)}
              fullWidth
            />

            <div class={styles.filterRow}>
              <div class={styles.filterField}>
                <label class={styles.filterLabel}>Status</label>
                <Select
                  value={status()}
                  onChange={(e) => handleFilterChange("status", e.currentTarget.value || null)}
                >
                  <For each={getStatusOptions()}>
                    {(opt) => <option value={opt.value}>{opt.label}</option>}
                  </For>
                </Select>
              </div>

              <div class={styles.filterField}>
                <label class={styles.filterLabel}>Active</label>
                <Select
                  value={active()}
                  onChange={(e) => handleFilterChange("active", e.currentTarget.value || null)}
                >
                  <option value="">Any</option>
                  <option value="true">Yes</option>
                  <option value="false">No</option>
                </Select>
              </div>
            </div>

            <div class={styles.customParameter}>
              <span class={styles.customLabel}>Custom Parameter</span>
              <div class={styles.customInputs}>
                <Input
                  placeholder="Parameter name"
                  value={customParam()}
                  onInput={(e) => setCustomParam(e.currentTarget.value)}
                />
                <Input
                  placeholder="Value"
                  value={customValue()}
                  onInput={(e) => setCustomValue(e.currentTarget.value)}
                />
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={handleCustomParameterAdd}
                  disabled={!customParam() || !customValue()}
                >
                  <IconPlus size={14} />
                  Add
                </Button>
              </div>
            </div>
          </div>
        </Show>

        <Show when={hasActiveFilters()}>
          <div class={styles.activeFilters}>
            <span class={styles.activeLabel}>Active Filters:</span>
            <div class={styles.filterTags}>
              <For each={activeFilterEntries()}>
                {([key, value]) => (
                  <div class={styles.filterTag}>
                    <span class={styles.filterTagText}>
                      <strong>{key}:</strong> {value}
                    </span>
                    <button
                      class={styles.filterTagRemove}
                      onClick={() => handleFilterChange(key, null)}
                      title={`Remove ${key} filter`}
                    >
                      <IconX size={10} />
                    </button>
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>
      </div>
    </div>
  );
};

import {
  ActionIcon,
  Alert,
  Badge,
  Button,
  DataTable,
  type DataTableColumn,
  type DropdownMenuItems,
  EmptyState,
  getResourceDisplayName,
  notify,
  OperationOutcomePanel,
  ReferenceLink,
  Resizable,
  ScrollArea,
  SegmentedControl,
  SelectionBar,
  Skeleton,
  Text,
  TextInput,
} from "@octofhir/ui-kit";
import {
  ChevronLeft,
  ChevronRight,
  CircleAlert as CircleExclamation,
  Code,
  Copy,
  FileText,
  Hash,
  Search as Magnifier,
  Send,
  Trash2,
  X as Xmark,
} from "lucide-react";
import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import {
  type FhirCatalogCategoryFilter,
  filterFhirCatalogTypes,
  getFhirCatalogCategoryOptions,
  getFhirCatalogTypeViews,
} from "@/entities/fhir-catalog";
import { getFhirBundleResources } from "@/entities/fhir-resource";
import { HttpError } from "@/shared/api/fhirClient";
import { assertFhirResource, isRecord } from "@/shared/api/guards";
import { isTypingTarget } from "@/shared/lib/keyboard";
import {
  useDeleteResource,
  useFollowBundleLink,
  useJsonSchema,
  useResource,
  useResourceSearch,
  useResourceTypesCategorized,
  useUpdateResource,
} from "@/shared/api/hooks";
import type { FhirBundle, FhirOperationOutcome, FhirResource } from "@/shared/api/types";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import classes from "./ResourceBrowserPage.module.css";

dayjs.extend(relativeTime);

const CATEGORY_SECTIONS = [
  { id: "fhir", label: "FHIR" },
  { id: "system", label: "System" },
  { id: "custom", label: "Custom" },
] as const;

type BadgeTheme = "success" | "warning" | "danger" | "normal";

const STATUS_THEME: Record<string, BadgeTheme> = {
  active: "success",
  final: "success",
  completed: "success",
  amended: "success",
  draft: "warning",
  preliminary: "warning",
  registered: "warning",
  "on-hold": "warning",
  "entered-in-error": "danger",
  cancelled: "danger",
  revoked: "danger",
};

/** Columns dropped while the details pane takes half of the width. */
const SPLIT_HIDDEN_COLUMNS = new Set(["version", "updated"]);

interface ResourceRow {
  id: string;
  resource: FhirResource;
}

/** Parses `name=anna&_sort=-birthdate` into search params; a leading `?` is allowed. */
function parseQuery(text: string): Record<string, string> {
  const params: Record<string, string> = {};
  for (const [key, value] of new URLSearchParams(text.trim().replace(/^\?/, ""))) {
    if (key) params[key] = value;
  }
  return params;
}

function resourceStatus(resource: FhirResource): string | undefined {
  const record = resource as Record<string, unknown>;
  if (typeof record.status === "string") return record.status;
  if (typeof record.active === "boolean") return record.active ? "active" : "inactive";
  return undefined;
}

function isCatalogCategoryFilter(value: string): value is FhirCatalogCategoryFilter {
  return value === "all" || value === "fhir" || value === "system" || value === "custom";
}

/** A deleted resource as it can be PUT back: server-owned version fields go, and so does an emptied meta. */
function restorable(resource: FhirResource): FhirResource {
  const { meta: original, ...rest } = resource;
  const { versionId: _version, lastUpdated: _updated, ...meta } = original ?? {};
  return Object.keys(meta).length ? { ...rest, meta } : rest;
}

function isOperationOutcome(value: unknown): value is FhirOperationOutcome {
  return isRecord(value) && value.resourceType === "OperationOutcome";
}

function getErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

export function ResourceBrowserPage() {
  const { type: routeType, id: routeId } = useParams<{ type?: string; id?: string }>();
  const navigate = useNavigate();

  const selectedType = routeType ?? null;
  const selectedId = routeId ?? null;
  const [typeFilter, setTypeFilter] = useState("");
  const [categoryFilter, setCategoryFilter] = useState<FhirCatalogCategoryFilter>("all");
  const searchRef = useRef<HTMLInputElement>(null);
  const [isEditMode, setIsEditMode] = useState(false);
  const [editedResource, setEditedResource] = useState("");
  const [currentBundle, setCurrentBundle] = useState<FhirBundle | null>(null);
  // The applied search lives in the URL (`?q=name=anna`) so it survives reloads and can be shared.
  const [urlParams, setUrlParams] = useSearchParams();
  const appliedQuery = urlParams.get("q") ?? "";
  const [queryText, setQueryText] = useState(appliedQuery);
  const searchParams = useMemo(() => ({ _count: 50, ...parseQuery(appliedQuery) }), [appliedQuery]);
  // Rows ticked for bulk actions (x or the checkbox); separate from the row open in the panel.
  const [checked, setChecked] = useState<string[]>([]);
  const [saveError, setSaveError] = useState<{
    message: string;
    operationOutcome?: FhirOperationOutcome;
  } | null>(null);

  const {
    data: categorizedTypes,
    isLoading: resourceTypesLoading,
    isError: resourceTypesError,
    error: resourceTypesErrorObj,
    refetch: refetchResourceTypes,
  } = useResourceTypesCategorized();
  const {
    data: searchBundle,
    isLoading: searchLoading,
    isError: searchError,
    error: searchErrorObj,
    refetch: refetchSearch,
  } = useResourceSearch(selectedType ?? "", searchParams, { enabled: !!selectedType });
  const {
    data: selectedResource,
    isLoading: resourceLoading,
    isError: resourceError,
    error: resourceErrorObj,
    refetch: refetchResource,
  } = useResource(selectedType ?? "", selectedId ?? "", {
    enabled: !!selectedType && !!selectedId,
  });
  const { data: jsonSchema } = useJsonSchema(selectedType ?? undefined);
  const jsonSchemaObject = isRecord(jsonSchema) ? jsonSchema : undefined;
  const updateMutation = useUpdateResource();
  const followLinkMutation = useFollowBundleLink();

  // Reset browser state when the route resource type changes
  // biome-ignore lint/correctness/useExhaustiveDependencies: resetting state intentionally keys off selectedType
  useEffect(() => {
    setCurrentBundle(null);
    setIsEditMode(false);
    setSaveError(null);
    setChecked([]);
  }, [selectedType]);

  useEffect(() => {
    setQueryText(appliedQuery);
    setChecked([]);
  }, [appliedQuery]);

  // Update current bundle when search bundle changes
  useEffect(() => {
    if (searchBundle) {
      setCurrentBundle(searchBundle);
    }
  }, [searchBundle]);

  // Update edited resource when selected resource changes
  useEffect(() => {
    if (selectedResource) {
      setEditedResource(JSON.stringify(selectedResource, null, 2));
      setIsEditMode(false);
    }
  }, [selectedResource]);

  // Global "/" shortcut focuses the type search (only on the catalog level)
  useEffect(() => {
    if (selectedType) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "/" || event.metaKey || event.ctrlKey || event.altKey) return;
      const target = event.target as HTMLElement | null;
      const tag = target?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable) return;
      event.preventDefault();
      searchRef.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [selectedType]);

  // Memoize category filter data to avoid infinite re-renders
  const categoryFilterData = useMemo(
    () => [...getFhirCatalogCategoryOptions(categorizedTypes)],
    [categorizedTypes]
  );

  // Filter resource types by category and search
  const filteredTypes = useMemo(() => {
    return filterFhirCatalogTypes(categorizedTypes, categoryFilter, typeFilter);
  }, [categorizedTypes, categoryFilter, typeFilter]);
  const filteredTypeViews = useMemo(() => getFhirCatalogTypeViews(filteredTypes), [filteredTypes]);

  // Extract resources from current bundle
  const resources = useMemo(() => {
    return getFhirBundleResources(currentBundle);
  }, [currentBundle]);
  const resourceRows = useMemo<ResourceRow[]>(
    () =>
      resources.flatMap((resource) => (resource.id ? [{ id: resource.id, resource }] : [])),
    [resources]
  );
  const typeSections = useMemo(
    () =>
      CATEGORY_SECTIONS.map((section) => ({
        ...section,
        items: filteredTypeViews.filter((item) => item.category === section.id),
      })).filter((section) => section.items.length > 0),
    [filteredTypeViews]
  );

  // Pagination state
  const hasNextPage = currentBundle?.link?.some((l) => l.relation === "next") ?? false;
  const hasPrevPage = currentBundle?.link?.some((l) => l.relation === "prev") ?? false;

  const handleTypeSelect = (type: string) => {
    setCurrentBundle(null);
    setIsEditMode(false);
    setSaveError(null);
    navigate(`/resources/${type}`);
  };

  const handleResourceSelect = (id: string) => {
    if (!selectedType) return;
    navigate(`/resources/${selectedType}/${id}`);
  };

  const deleteMutation = useDeleteResource();

  const copyText = (text: string, what: string) => {
    void navigator.clipboard.writeText(text).then(() =>
      notify({
        theme: "success",
        title: `Copied ${what}`,
        content: text.includes("\n") ? undefined : text,
        autoHiding: 2500,
      })
    );
  };

  // Linear-style delete: act at once, offer Undo (a PUT brings the resource back under its id).
  // Linear-style delete: act at once, offer Undo (a PUT brings each resource back under its id).
  const deleteWithUndo = async (targets: FhirResource[]) => {
    const results = await Promise.allSettled(
      targets.map((resource) =>
        deleteMutation.mutateAsync({ resourceType: resource.resourceType, id: resource.id ?? "" }).then(() => resource)
      )
    );
    const deleted = results.flatMap((result) => (result.status === "fulfilled" ? [result.value] : []));
    const failed = results.length - deleted.length;
    if (failed > 0) {
      notify({ theme: "danger", title: `${failed} of ${results.length} not deleted` });
    }
    if (deleted.length === 0) return;
    if (deleted.some((resource) => resource.id === selectedId)) navigate(`/resources/${selectedType}`);
    setChecked([]);

    const single = deleted.length === 1 ? deleted[0] : null;
    const label = single
      ? getResourceDisplayName(single, `${single.resourceType}/${single.id}`)
      : `${deleted.length} resources`;
    notify({
      title: `Deleted ${label}`,
      content: single ? `${single.resourceType}/${single.id}` : undefined,
      autoHiding: 8000,
      action: {
        label: "Undo",
        onClick: () => {
          Promise.all(deleted.map((resource) => updateMutation.mutateAsync(restorable(resource))))
            .then(() => notify({ theme: "success", title: `Restored ${label}`, autoHiding: 2500 }))
            .catch((error: unknown) =>
              notify({ theme: "danger", title: "Restore failed", content: getErrorMessage(error, label) })
            );
        },
      },
    });
  };

  const resourceRowMenu = (row: ResourceRow): DropdownMenuItems => {
    const address = `${row.resource.resourceType}/${row.id}`;
    return [
      [
        { text: "Open", iconStart: <FileText size={14} />, action: () => handleResourceSelect(row.id) },
        {
          text: "Open in Requests",
          iconStart: <Send size={14} />,
          action: () => navigate(`/console?${new URLSearchParams({ url: `/fhir/${address}` }).toString()}`),
        },
      ],
      [
        { text: "Copy reference", iconStart: <Copy size={14} />, action: () => copyText(address, "reference") },
        { text: "Copy id", iconStart: <Hash size={14} />, action: () => copyText(row.id, "id") },
      ],
      [
        {
          text: "Delete",
          iconStart: <Trash2 size={14} />,
          theme: "danger",
          action: () => void deleteWithUndo([row.resource]),
        },
      ],
    ];
  };

  const handleCloseDetails = () => {
    setIsEditMode(false);
    setSaveError(null);
    if (selectedType) {
      navigate(`/resources/${selectedType}`);
      return;
    }
    navigate("/resources");
  };

  // Detail-panel keys (Linear-style): j/k step through the list, e edits, Esc closes, ⌘S saves.
  // Capture phase: a focused table row also handles j/k (moving focus), and the open
  // resource should follow it rather than be blocked by it.
  useEffect(() => {
    if (!selectedId) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        if (!isEditMode) return;
        event.preventDefault();
        void handleSave();
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey || isEditMode) return;
      if (isTypingTarget(event.target) || document.querySelector("[role='dialog'], [role='menu']")) return;
      if (event.key === "Escape") {
        event.preventDefault();
        handleCloseDetails();
      } else if (event.key === "e") {
        event.preventDefault();
        setSaveError(null);
        setIsEditMode(true);
      } else if (event.key === "j" || event.key === "k") {
        const index = resourceRows.findIndex((row) => row.id === selectedId);
        const next = resourceRows[index + (event.key === "j" ? 1 : -1)];
        if (next) handleResourceSelect(next.id);
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  });

  const handleNextPage = async () => {
    if (currentBundle) {
      const result = await followLinkMutation.mutateAsync({
        bundle: currentBundle,
        relation: "next",
      });
      if (result) {
        setCurrentBundle(result);
        if (selectedType) {
          navigate(`/resources/${selectedType}`);
        }
      }
    }
  };

  const handlePrevPage = async () => {
    if (currentBundle) {
      const result = await followLinkMutation.mutateAsync({
        bundle: currentBundle,
        relation: "prev",
      });
      if (result) {
        setCurrentBundle(result);
        if (selectedType) {
          navigate(`/resources/${selectedType}`);
        }
      }
    }
  };

  const handleSave = async () => {
    setSaveError(null);
    try {
      const parsed = assertFhirResource(JSON.parse(editedResource), "Resource editor save");
      await updateMutation.mutateAsync(parsed);
      setIsEditMode(false);
      notify({
        theme: "success",
        title: "Success",
        content: "Resource updated successfully",
      });
    } catch (error) {
      if (error instanceof HttpError) {
        if (isOperationOutcome(error.response.data)) {
          setSaveError({
            message: error.message,
            operationOutcome: error.response.data,
          });
        } else {
          setSaveError({ message: error.message });
        }
      } else {
        const errorMessage = error instanceof Error ? error.message : "Failed to update resource";
        setSaveError({ message: errorMessage });
      }
    }
  };

  const handleCancel = () => {
    if (selectedResource) {
      setEditedResource(JSON.stringify(selectedResource, null, 2));
    }
    setIsEditMode(false);
  };

  const resourceColumns: DataTableColumn<ResourceRow>[] = [
    {
      id: "id",
      header: "id",
      accessor: (row) => row.id,
      width: 260,
      cell: (row) => (
        <ReferenceLink
          reference={`${row.resource.resourceType}/${row.id}`}
          href={`${import.meta.env.BASE_URL}resources/${row.resource.resourceType}/${row.id}`}
          onClick={(event) => {
            event.preventDefault();
            handleResourceSelect(row.id);
          }}
        />
      ),
    },
    {
      id: "display",
      header: "Name",
      accessor: (row) => getResourceDisplayName(row.resource, ""),
      cell: (row) => {
        const name = getResourceDisplayName(row.resource, "");
        return name && name !== row.id ? (
          <span className={classes.displayName}>{name}</span>
        ) : (
          <span className={classes.muted}>—</span>
        );
      },
    },
    {
      id: "status",
      header: "Status",
      width: 140,
      accessor: (row) => resourceStatus(row.resource) ?? "",
      cell: (row) => {
        const status = resourceStatus(row.resource);
        return status ? (
          <Badge theme={STATUS_THEME[status] ?? "normal"} size="sm">
            {status}
          </Badge>
        ) : (
          <span className={classes.muted}>—</span>
        );
      },
    },
    {
      id: "version",
      header: "Ver",
      width: 64,
      align: "right",
      accessor: (row) => row.resource.meta?.versionId ?? "",
      cell: (row) => <span className={classes.mono}>{row.resource.meta?.versionId ?? "—"}</span>,
    },
    {
      id: "updated",
      header: "Updated",
      width: 130,
      accessor: (row) => row.resource.meta?.lastUpdated ?? "",
      cell: (row) => {
        const updated = row.resource.meta?.lastUpdated;
        return updated ? (
          <span className={classes.muted} title={new Date(updated).toLocaleString()}>
            {dayjs(updated).fromNow()}
          </span>
        ) : (
          <span className={classes.muted}>—</span>
        );
      },
    },
  ];

  // Level 1: resource type catalog, grouped by category so no legend is needed
  const renderResourceTypesTable = () => (
    <div className={classes.catalogPanel}>
      <div className={classes.catalogToolbar}>
        <SegmentedControl
          size="sm"
          value={categoryFilter}
          onUpdate={(val) => {
            if (isCatalogCategoryFilter(val)) {
              setCategoryFilter(val);
            }
          }}
          options={categoryFilterData.map((option) => ({
            value: option.value,
            content: option.label,
          }))}
        />
        <TextInput
          ref={searchRef}
          placeholder="Search resource types…"
          aria-label="Search resource types"
          leftSection={<Magnifier width={14} height={14} aria-hidden="true" />}
          rightSection={
            typeFilter ? (
              <ActionIcon
                variant="subtle"
                size="xs"
                aria-label="Clear search"
                onClick={() => setTypeFilter("")}
              >
                <Xmark width={12} height={12} aria-hidden="true" />
              </ActionIcon>
            ) : (
              <kbd className={classes.kbdHint}>/</kbd>
            )
          }
          value={typeFilter}
          onChange={(value) => setTypeFilter(value)}
          onKeyDown={(event) => {
            if (event.key === "Escape" && typeFilter) {
              event.preventDefault();
              setTypeFilter("");
            } else if (event.key === "Enter" && filteredTypeViews.length > 0) {
              handleTypeSelect(filteredTypeViews[0].id);
            }
          }}
          className={classes.searchInput}
        />
        <span className={classes.resultCount}>
          {filteredTypes.length} {filteredTypes.length === 1 ? "type" : "types"}
        </span>
      </div>

      {resourceTypesLoading ? (
        <div className={classes.typeGrid} aria-hidden="true">
          {Array.from({ length: 24 }, (_, index) => `tile-skeleton-${index}`).map((key) => (
            <Skeleton key={key} className={classes.tileSkeleton} />
          ))}
        </div>
      ) : resourceTypesError ? (
        <EmptyState
          className={classes.stateBlock}
          image={<CircleExclamation width={40} height={40} aria-hidden="true" />}
          title="Failed to load resource types"
          description={getErrorMessage(
            resourceTypesErrorObj,
            "Could not load the FHIR resource catalog."
          )}
          actions={[
            <Button key="retry" variant="filled" onClick={() => refetchResourceTypes()}>
              Retry
            </Button>,
          ]}
        />
      ) : filteredTypes.length === 0 ? (
        typeFilter || categoryFilter !== "all" ? (
          <EmptyState
            className={classes.stateBlock}
            image={<Magnifier width={40} height={40} aria-hidden="true" />}
            title="No matching resource types"
            description="No resource types match the current search and filter."
            actions={[
              <Button
                key="clear"
                variant="outline"
                onClick={() => {
                  setTypeFilter("");
                  setCategoryFilter("all");
                }}
              >
                Clear filters
              </Button>,
            ]}
          />
        ) : (
          <EmptyState
            className={classes.stateBlock}
            image={<FileText width={40} height={40} aria-hidden="true" />}
            title="No resource types found"
            description="The FHIR catalog is empty. Load an Implementation Guide to add resource types."
          />
        )
      ) : (
        <ScrollArea className={`${classes.scrollArea} custom-scrollbar`}>
          {typeSections.map((section) => (
            <section key={section.id} className={classes.typeSection}>
              <h2 className={classes.sectionLabel}>
                {section.label} <span>{section.items.length}</span>
              </h2>
              <div className={classes.typeGrid}>
                {section.items.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    className={classes.typeTile}
                    title={item.definitionUrl ?? item.name}
                    onClick={() => handleTypeSelect(item.id)}
                  >
                    <span className={classes.typeName}>{item.name}</span>
                    {section.id !== "fhir" && (
                      <span className={classes.typePackage}>{item.packageName}</span>
                    )}
                  </button>
                ))}
              </div>
            </section>
          ))}
        </ScrollArea>
      )}
    </div>
  );

  // Level 2: resources of the selected type
  const checkedResources = resourceRows.filter((row) => checked.includes(row.id)).map((row) => row.resource);

  const renderResourcesTable = () => (
    <div className={classes.tablePanel}>
      <SelectionBar
        count={checkedResources.length}
        onClear={() => setChecked([])}
        actions={[
          {
            label: "Copy references",
            icon: <Copy size={14} />,
            onClick: () =>
              copyText(
                checkedResources.map((resource) => `${resource.resourceType}/${resource.id}`).join("\n"),
                `${checkedResources.length} references`
              ),
          },
          {
            label: "Delete",
            icon: <Trash2 size={14} />,
            danger: true,
            onClick: () => void deleteWithUndo(checkedResources),
          },
        ]}
      />
      <div className={classes.listToolbar}>
        <form
          className={classes.queryForm}
          onSubmit={(event) => {
            event.preventDefault();
            const next = queryText.trim();
            setUrlParams(next ? { q: next } : {});
          }}
        >
          <TextInput
            aria-label={`Search ${selectedType} with FHIR search parameters`}
            placeholder="name=anna&_sort=-_lastUpdated"
            leftSection={<span className={classes.queryPrefix}>?</span>}
            value={queryText}
            onChange={(value) => setQueryText(value)}
            className={classes.queryInput}
          />
        </form>
        <span className={classes.resultCount}>
          {currentBundle?.total ?? resourceRows.length} total
        </span>
        {(hasNextPage || hasPrevPage) && (
          <div className={classes.paginationActions}>
            <ActionIcon
              variant="subtle"
              size="sm"
              aria-label="Previous page"
              disabled={!hasPrevPage || followLinkMutation.isPending}
              onClick={handlePrevPage}
            >
              <ChevronLeft width={16} height={16} aria-hidden="true" />
            </ActionIcon>
            <ActionIcon
              variant="subtle"
              size="sm"
              aria-label="Next page"
              disabled={!hasNextPage || followLinkMutation.isPending}
              onClick={handleNextPage}
            >
              <ChevronRight width={16} height={16} aria-hidden="true" />
            </ActionIcon>
          </div>
        )}
      </div>

      {searchError ? (
        <EmptyState
          className={classes.stateBlock}
          image={<CircleExclamation width={40} height={40} aria-hidden="true" />}
          title="Search failed"
          description={getErrorMessage(
            searchErrorObj,
            `Could not search ${selectedType ?? "resources"}.`
          )}
          actions={[
            <Button key="retry" variant="filled" onClick={() => refetchSearch()}>
              Retry
            </Button>,
          ]}
        />
      ) : (
        <DataTable
          aria-label={`${selectedType} resources`}
          data={resourceRows}
          columns={
            selectedId
              ? resourceColumns.filter((column) => !SPLIT_HIDDEN_COLUMNS.has(column.id))
              : resourceColumns
          }
          getRowId={(row) => row.id}
          loading={searchLoading}
          loadingRowCount={8}
          stickyHeader
          fillHeight
          selectable
          selectedRowIds={checked}
          onSelectedRowIdsChange={setChecked}
          activeRowId={selectedId ?? undefined}
          onRowClick={(row) => handleResourceSelect(row.id)}
          rowMenu={resourceRowMenu}
          emptyState={
            <EmptyState
              className={classes.stateBlock}
              image={<FileText width={40} height={40} aria-hidden="true" />}
              title={appliedQuery ? "No matches" : "No resources yet"}
              description={
                appliedQuery
                  ? `No ${selectedType} matches ?${appliedQuery}.`
                  : `There are no ${selectedType ?? "resources"} stored yet.`
              }
            />
          }
        />
      )}
    </div>
  );

  // Details Panel Content without manual resize handle
  const renderDetailsPanelContent = () => (
    <div className={classes.detailsPanel}>
      <div className={classes.panelHeader}>
        <div className={classes.toolbar}>
          <div className={classes.resourceIdentity}>
            <ReferenceLink reference={`${selectedType}/${selectedId}`} />
          </div>
          <div className={classes.detailActions}>
            {isEditMode ? (
              <div className={classes.editActions}>
                <Button
                  size="xs"
                  variant="subtle"
                  onClick={handleCancel}
                  disabled={updateMutation.isPending}
                >
                  Cancel
                </Button>
                <Button
                  size="xs"
                  variant="filled"
                  onClick={handleSave}
                  loading={updateMutation.isPending}
                >
                  Save Resource
                </Button>
              </div>
            ) : (
              <Button
                size="xs"
                variant="outline"
                onClick={() => {
                  setSaveError(null);
                  setIsEditMode(true);
                }}
              >
                <Button.Icon side="left">
                  <Code width={14} height={14} aria-hidden="true" />
                </Button.Icon>
                Edit JSON
              </Button>
            )}
            <ActionIcon
              variant="subtle"
              size="md"
              aria-label="Close resource details"
              onClick={handleCloseDetails}
            >
              <Xmark width={16} height={16} aria-hidden="true" />
            </ActionIcon>
          </div>
        </div>
      </div>

      {resourceLoading ? (
        <div className={classes.detailsLoading}>
          <Skeleton className={classes.detailsSkeleton} />
        </div>
      ) : resourceError ? (
        <EmptyState
          className={classes.stateBlock}
          image={<CircleExclamation width={48} height={48} aria-hidden="true" />}
          title="Failed to load resource"
          description={getErrorMessage(
            resourceErrorObj,
            `Could not load ${selectedType ?? "resource"}/${selectedId ?? ""}.`
          )}
          actions={[
            <Button key="retry" variant="filled" onClick={() => refetchResource()}>
              Retry
            </Button>,
          ]}
        />
      ) : (
        <div className={classes.detailsBody}>
          <div className={classes.editorFill}>
            <JsonEditor
              value={editedResource}
              onChange={isEditMode ? setEditedResource : undefined}
              readOnly={!isEditMode}
              height="100%"
              schema={jsonSchemaObject}
              resourceType={selectedType ?? undefined}
            />
          </div>
          {saveError && (
            <div className={classes.errorPanel}>
              <Alert
                theme="danger"
                title={saveError.message}
                message={
                  saveError.operationOutcome?.issue ? (
                    <OperationOutcomePanel outcome={saveError.operationOutcome} maxIssues={3} />
                  ) : (
                    <Text variant="caption-2">An error occurred while saving.</Text>
                  )
                }
              />
            </div>
          )}
        </div>
      )}
    </div>
  );

  return (
    <ToolWorkspaceLayout
      title={selectedType ?? "Resources"}
      description={selectedType ? undefined : "Browse, search and edit stored FHIR resources."}
      className="page-enter"
    >
      <div className={classes.pageBody}>
        {!selectedType ? (
          // Level 1: Resource Types Table
          renderResourceTypesTable()
        ) : (
          // Level 2: Resources Table (+ optional Details Panel)
          <div className={classes.splitView}>
            <Resizable.Group orientation="horizontal">
              <Resizable.Pane defaultSize={selectedId ? 50 : 100} minSize={30}>
                {renderResourcesTable()}
              </Resizable.Pane>

              {selectedId && (
                <>
                  <Resizable.Handle />
                  <Resizable.Pane defaultSize={50} minSize={30}>
                    <div className={classes.detailsShell}>
                      {renderDetailsPanelContent()}
                    </div>
                  </Resizable.Pane>
                </>
              )}
            </Resizable.Group>
          </div>
        )}
      </div>
    </ToolWorkspaceLayout>
  );
}

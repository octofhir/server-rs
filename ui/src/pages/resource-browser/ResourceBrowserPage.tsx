import {
  ActionIcon,
  Alert,
  Anchor,
  Badge,
  Breadcrumbs,
  Button,
  EmptyState,
  notify,
  OperationOutcomePanel,
  RecordList,
  Resizable,
  ScrollArea,
  SegmentedControl,
  Skeleton,
  Text,
  TextInput,
} from "@octofhir/ui-kit";
import {
  ChevronLeft,
  ChevronRight,
  CircleAlert as CircleExclamation,
  Code,
  FileText,
  Search as Magnifier,
  X as Xmark,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import {
  type FhirCatalogCategoryFilter,
  filterFhirCatalogTypes,
  getFhirCatalogCategoryOptions,
  getFhirCatalogTypeViews,
} from "@/entities/fhir-catalog";
import { getFhirBundleResources, getFhirResourceListViews } from "@/entities/fhir-resource";
import { HttpError } from "@/shared/api/fhirClient";
import { assertFhirResource, isRecord } from "@/shared/api/guards";
import {
  useFollowBundleLink,
  useJsonSchema,
  useResource,
  useResourceSearch,
  useResourceTypesCategorized,
  useUpdateResource,
} from "@/shared/api/hooks";
import type { FhirBundle, FhirOperationOutcome } from "@/shared/api/types";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import classes from "./ResourceBrowserPage.module.css";

function isCatalogCategoryFilter(value: string): value is FhirCatalogCategoryFilter {
  return value === "all" || value === "fhir" || value === "system" || value === "custom";
}

function isOperationOutcome(value: unknown): value is FhirOperationOutcome {
  return isRecord(value) && value.resourceType === "OperationOutcome";
}

function getErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

function ListSkeleton({ rows }: { rows: number }) {
  return (
    <div className={classes.skeletonList} aria-hidden="true">
      {Array.from({ length: rows }, (_, index) => `skeleton-${index}`).map((key) => (
        <Skeleton key={key} className={classes.skeletonRow} />
      ))}
    </div>
  );
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
  } = useResourceSearch(selectedType ?? "", { _count: 50 }, { enabled: !!selectedType });
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
  }, [selectedType]);

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
  const resourceViews = useMemo(() => getFhirResourceListViews(resources), [resources]);

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

  const handleBackToTypes = () => {
    setCurrentBundle(null);
    setIsEditMode(false);
    setSaveError(null);
    navigate("/resources");
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

  // Breadcrumb items
  const breadcrumbItems = [
    <Anchor key="root" onClick={handleBackToTypes} view="secondary">
      Resources
    </Anchor>,
  ];
  if (selectedType) {
    breadcrumbItems.push(
      <Text key="type" variant={selectedId ? "body-2" : "subheader-2"}>
        {selectedType}
      </Text>
    );
  }
  if (selectedId) {
    breadcrumbItems.push(
      <Text key="id" variant="subheader-2">
        {selectedId}
      </Text>
    );
  }

  // Resource Types Catalog — flat tile grid
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
          size="sm"
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
          {Array.from({ length: 18 }, (_, index) => `tile-skeleton-${index}`).map((key) => (
            <Skeleton key={key} className={classes.tileSkeleton} />
          ))}
        </div>
      ) : resourceTypesError ? (
        <EmptyState
          className={classes.stateBlock}
          image={<CircleExclamation width={48} height={48} aria-hidden="true" />}
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
            image={<Magnifier width={48} height={48} aria-hidden="true" />}
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
            image={<FileText width={48} height={48} aria-hidden="true" />}
            title="No resource types found"
            description="The FHIR catalog is empty. Load an Implementation Guide to add resource types."
          />
        )
      ) : (
        <ScrollArea className={`${classes.scrollArea} custom-scrollbar`}>
          <div className={classes.typeGrid}>
            {filteredTypeViews.map((item) => (
              <button
                key={item.id}
                type="button"
                className={classes.typeTile}
                data-category={item.category}
                title={item.definitionUrl ?? item.name}
                onClick={() => handleTypeSelect(item.id)}
              >
                <span className={classes.typeTileTop}>
                  <span className={classes.typeName}>{item.name}</span>
                  <span className={classes.typeCategoryDot} aria-hidden="true" />
                </span>
                <span className={classes.typePackage}>{item.packageName}</span>
              </button>
            ))}
          </div>
        </ScrollArea>
      )}
    </div>
  );

  // Resources Table
  const renderResourcesTable = () => (
    <div className={classes.tablePanel} style={{ height: "100%" }}>
      <div className={classes.panelHeader}>
        <div className={classes.toolbar}>
          <Text variant="body-2" color="secondary" className={classes.overline}>
            <strong>{selectedType}s</strong>
          </Text>
          <div className={classes.titleRow}>
            {currentBundle && (
              <Badge color="primary">{currentBundle.total ?? resources.length} Total</Badge>
            )}
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
        </div>
      </div>

      {searchLoading ? (
        <div className={classes.listPadding}>
          <ListSkeleton rows={8} />
        </div>
      ) : searchError ? (
        <EmptyState
          className={classes.stateBlock}
          image={<CircleExclamation width={48} height={48} aria-hidden="true" />}
          title="Failed to load resources"
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
      ) : resources.length === 0 ? (
        <EmptyState
          className={classes.stateBlock}
          image={<FileText width={48} height={48} aria-hidden="true" />}
          title="No resources found"
          description={`There are no ${selectedType ?? "resources"} stored yet.`}
        />
      ) : (
        <ScrollArea className={`${classes.scrollArea} custom-scrollbar`}>
          <div className={classes.listPadding}>
            <RecordList
              density="compact"
              selectedId={selectedId ?? undefined}
              items={resourceViews.map((resource) => ({
                id: resource.id,
                title: resource.resourceId ?? "(no id)",
                subtitle: resource.resourceType,
                description: resource.lastUpdatedLabel,
                disabled: !resource.canOpen,
                meta: [
                  {
                    id: "status",
                    label: resource.statusLabel,
                    tone: "neutral",
                  },
                  {
                    id: "version",
                    label: resource.versionLabel,
                    tone: "info",
                  },
                ],
              }))}
              onSelect={(item) => handleResourceSelect(item.id)}
            />
          </div>
        </ScrollArea>
      )}
    </div>
  );

  // Details Panel Content without manual resize handle
  const renderDetailsPanelContent = () => (
    <div className={classes.detailsPanel} style={{ width: "100%", height: "100%" }}>
      <div className={classes.panelHeader}>
        <div className={classes.toolbar}>
          <div className={classes.resourceIdentity}>
            <Badge color="primary">{selectedType}</Badge>
            <Text variant="body-2" color="secondary" className={classes.monospace}>
              <strong>{selectedId}</strong>
            </Text>
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
      title="Resource Browser"
      description="Browse, inspect, and edit FHIR resources"
      className="page-enter"
      kicker={
        <Breadcrumbs separator="→" className={classes.breadcrumbs}>
          {breadcrumbItems}
        </Breadcrumbs>
      }
      actions={
        selectedType && !selectedId ? (
          <Button variant="subtle" onClick={handleBackToTypes}>
            <Button.Icon side="left">
              <ChevronLeft width={16} height={16} aria-hidden="true" />
            </Button.Icon>
            Change Resource Type
          </Button>
        ) : null
      }
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
                    <div className={classes.detailsShell} style={{ flex: 1, minWidth: 0 }}>
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

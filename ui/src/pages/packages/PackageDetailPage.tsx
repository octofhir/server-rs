import { useMemo, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
  Text,
  Badge,
  DataPreview,
  Loader,
  Alert,
  TextInput,
  ActionIcon,
  Tooltip,
  Tabs,
  Button,
  Select,
  Breadcrumbs,
  Anchor,
  Modal,
  Code,
  useDebouncedValue,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
  getFhirPackageResourceTypeOptions,
  getFhirPackageResourceViews,
} from "@/entities/fhir-package";
import {
  CircleAlert as CircleExclamation,
  Search as Magnifier,
  ArrowLeft,
  Check,
  TriangleAlert as TriangleExclamation,
  Eye,
  File,
  Code as CodeIcon,
} from "lucide-react";
import {
  usePackageDetails,
  usePackageResources,
  usePackageResourceContent,
  usePackageFhirSchema,
} from "@/shared/api/hooks";
import type { PackageDetailResponse, PackageResourceSummary } from "@/shared/api/types";
import classes from "./PackageDetailPage.module.css";

function FhirVersionBadge({
  packageVersion,
  isCompatible,
}: {
  packageVersion?: string;
  isCompatible: boolean;
}) {
  return (
    <Tooltip label={isCompatible ? "Compatible with server" : "Version mismatch with server"}>
      <Badge
        size="md"
        variant="light"
        color={isCompatible ? "primary" : "warm"}
        leftSection={isCompatible ? <Check size={14} /> : <TriangleExclamation size={14} />}
      >
        FHIR {packageVersion || "unknown"}
      </Badge>
    </Tooltip>
  );
}

function ResourceTypeIcon({ resourceType }: { resourceType: string }) {
  return (
    <span className={classes.resourceTypeIcon} data-resource-type={resourceType}>
      <File size={14} />
    </span>
  );
}

interface ResourceViewerProps {
  packageName: string;
  packageVersion: string;
  resource: PackageResourceSummary;
  onClose: () => void;
}

function ResourceViewer({ packageName, packageVersion, resource, onClose }: ResourceViewerProps) {
  const [activeTab, setActiveTab] = useState<string | null>("json");
  const resourceUrl = resource.url || resource.id || "";

  const { data: content, isLoading: contentLoading } = usePackageResourceContent(
    packageName,
    packageVersion,
    resourceUrl
  );

  const { data: fhirSchema, isLoading: schemaLoading } = usePackageFhirSchema(
    packageName,
    packageVersion,
    resourceUrl
  );

  return (
    <Modal
      opened
      onClose={onClose}
      title={
        <div className={classes.modalTitle}>
          <ResourceTypeIcon resourceType={resource.resourceType} />
          <Text fw={500}>{resource.name || resource.id || resourceUrl}</Text>
        </div>
      }
      size="xl"
    >
      <Tabs value={activeTab} onChange={setActiveTab}>
        <Tabs.List>
          <Tabs.Tab value="json" leftSection={<CodeIcon width={14} />}>
            JSON
          </Tabs.Tab>
          {resource.resourceType === "StructureDefinition" && (
            <Tabs.Tab value="fhirschema" leftSection={<CodeIcon width={14} />}>
              FHIRSchema
            </Tabs.Tab>
          )}
        </Tabs.List>

        <Tabs.Panel value="json" p="md">
          {contentLoading ? (
            <div className={classes.modalState}>
              <Loader size="sm" />
              <Text size="sm" c="dimmed">
                Loading resource...
              </Text>
            </div>
          ) : content ? (
            <div className={classes.codeScroll}>
              <Code block className={classes.codeBlockSmall}>
                {JSON.stringify(content, null, 2)}
              </Code>
            </div>
          ) : (
            <Text c="dimmed">Failed to load resource content</Text>
          )}
        </Tabs.Panel>

        {resource.resourceType === "StructureDefinition" && (
          <Tabs.Panel value="fhirschema" p="md">
            {schemaLoading ? (
              <div className={classes.modalState}>
                <Loader size="sm" />
                <Text size="sm" c="dimmed">
                  Loading FHIRSchema...
                </Text>
              </div>
            ) : fhirSchema ? (
              <div className={classes.codeScroll}>
                <Code block className={classes.codeBlockSmall}>
                  {JSON.stringify(fhirSchema, null, 2)}
                </Code>
              </div>
            ) : (
              <Text c="dimmed">FHIRSchema not available for this resource</Text>
            )}
          </Tabs.Panel>
        )}
      </Tabs>
    </Modal>
  );
}

function ResourcesTab({
  packageName,
  packageVersion,
  resourceTypes,
  filterType,
  onFilterTypeChange,
}: {
  packageName: string;
  packageVersion: string;
  resourceTypes: Array<{ resourceType: string; count: number }>;
  filterType: string | null;
  onFilterTypeChange: (value: string | null) => void;
}) {
  const [search, setSearch] = useState("");
  const [selectedResource, setSelectedResource] = useState<PackageResourceSummary | null>(null);

  const RESULT_LIMIT = 500;
  // Debounce so we don't hit the backend on every keystroke.
  const debouncedSearch = useDebouncedValue(search.trim(), 300);

  // Search is performed server-side so it spans the whole package, not just
  // the first page of resources.
  const { data, isLoading, isFetching, error } = usePackageResources(packageName, packageVersion, {
    resourceType: filterType || undefined,
    search: debouncedSearch || undefined,
    limit: RESULT_LIMIT,
  });

  const filteredResources = data?.resources ?? [];
  const total = data?.total ?? 0;
  const truncated = total > filteredResources.length;
  const resourceViews = useMemo(
    () => getFhirPackageResourceViews(filteredResources),
    [filteredResources]
  );
  const typeOptions = useMemo(
    () => getFhirPackageResourceTypeOptions(resourceTypes),
    [resourceTypes]
  );

  // Memoized so typing in the search box doesn't rebuild 500 row elements
  // on every keystroke (only rebuilds when the fetched data changes).
  const rows = useMemo(
    () =>
      resourceViews.map((resource, index) => {
        const open = () => {
          const selected = filteredResources[index];
          if (selected) setSelectedResource(selected);
        };
        return {
          type: (
            <div className={classes.resourceCell}>
              <ResourceTypeIcon resourceType={resource.resourceType} />
              <Text size="sm">{resource.resourceType}</Text>
              {resource.classification === "base" && (
                <Badge size="xs" variant="light" color="green" title="Base resource / type">
                  Base
                </Badge>
              )}
              {resource.classification === "profile" && (
                <Badge size="xs" variant="light" color="indigo" title="Profile (constraint)">
                  Profile
                </Badge>
              )}
            </div>
          ),
          name: (
            <button type="button" className={classes.nameButton} onClick={open}>
              {resource.nameLabel}
            </button>
          ),
          url: (
            <Text size="xs" c="dimmed" className={classes.truncateText}>
              {resource.urlLabel}
            </Text>
          ),
          version: <Text size="sm">{resource.versionLabel}</Text>,
          actions: (
            <Tooltip label="View resource">
              <ActionIcon variant="subtle" size="sm" onClick={open}>
                <Eye size={16} />
              </ActionIcon>
            </Tooltip>
          ),
        };
      }),
    [resourceViews, filteredResources]
  );

  return (
    <div className={classes.tabStack}>
      <div className={classes.filters}>
        <TextInput
          placeholder="Search by name, URL or id (e.g. Patient)…"
          leftSection={<Magnifier size={16} />}
          rightSection={isFetching && !isLoading ? <Loader size="xs" /> : undefined}
          value={search}
          onChange={(value) => setSearch(value)}
          className={classes.searchInput}
        />
        <Select
          placeholder="Filter by type"
          data={typeOptions}
          value={filterType}
          onChange={onFilterTypeChange}
          clearable
          className={classes.typeSelect}
        />
      </div>

      {!isLoading && !error && (
        <div className={classes.resultsMeta}>
          <Text size="xs" c="dimmed">
            {total === 0
              ? "No matches"
              : truncated
                ? `Showing ${filteredResources.length} of ${total} matches`
                : `${total} ${total === 1 ? "resource" : "resources"}`}
          </Text>
          {truncated && (
            <Text size="xs" c="dimmed">
              Refine your search to narrow results
            </Text>
          )}
        </div>
      )}

      {isLoading && (
        <div className={classes.statePanel}>
          <Loader size="sm" />
          <Text size="sm" c="dimmed">
            Loading resources...
          </Text>
        </div>
      )}

      {error && (
        <Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
          {error instanceof Error ? error.message : "Failed to load resources"}
        </Alert>
      )}

      {!isLoading && !error && (
        <div className={classes.tablePanel}>
          <DataPreview
            columns={[
              { id: "type", label: "Type", width: 220 },
              { id: "name", label: "Name", width: 240 },
              { id: "url", label: "URL" },
              { id: "version", label: "Version", width: 120 },
              { id: "actions", label: "", width: 50 },
            ]}
            rows={rows}
            emptyText="No resources found"
            getRowKey={(_row, index) => resourceViews[index]?.id ?? `${index}`}
          />
        </div>
      )}

      {selectedResource && (
        <ResourceViewer
          packageName={packageName}
          packageVersion={packageVersion}
          resource={selectedResource}
          onClose={() => setSelectedResource(null)}
        />
      )}
    </div>
  );
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className={classes.statCard}>
      <Text size="xs" c="dimmed" className={classes.statLabel}>
        {label}
      </Text>
      <Text className={classes.statValue}>{value}</Text>
    </div>
  );
}

function PackageStats({
  data,
  onSelectType,
}: {
  data: PackageDetailResponse;
  onSelectType: (type: string) => void;
}) {
  const sortedTypes = useMemo(
    () => [...data.resourceTypes].sort((a, b) => b.count - a.count),
    [data.resourceTypes]
  );
  const maxCount = sortedTypes[0]?.count ?? 0;
  const total = data.resourceCount || sortedTypes.reduce((sum, rt) => sum + rt.count, 0);

  return (
    <div className={classes.tabStack}>
      {data.description && (
        <div className={classes.panel}>
          <Text size="sm" fw={500} c="dimmed" mb={4}>
            Description
          </Text>
          <Text size="sm">{data.description}</Text>
        </div>
      )}

      <div className={classes.statGrid}>
        <StatCard label="Total resources" value={total.toLocaleString()} />
        <StatCard label="Resource types" value={String(sortedTypes.length)} />
        <StatCard label="FHIR version" value={data.fhirVersion || "—"} />
        <StatCard
          label="Installed"
          value={data.installedAt ? new Date(data.installedAt).toLocaleDateString() : "—"}
        />
      </div>

      <div className={classes.panelMuted}>
        <div className={classes.breakdownHeader}>
          <Text size="sm" fw={500}>
            Resource types breakdown
          </Text>
          <Text size="xs" c="dimmed">
            Click a type to browse its resources
          </Text>
        </div>
        <div className={classes.typeBreakdown}>
          {sortedTypes.map((rt) => {
            const pct = total > 0 ? (rt.count / total) * 100 : 0;
            const barPct = maxCount > 0 ? (rt.count / maxCount) * 100 : 0;
            return (
              <button
                key={rt.resourceType}
                type="button"
                className={classes.typeRow}
                onClick={() => onSelectType(rt.resourceType)}
              >
                <div className={classes.typeRowTop}>
                  <span className={classes.typeName}>
                    <ResourceTypeIcon resourceType={rt.resourceType} />
                    {rt.resourceType}
                  </span>
                  <span className={classes.typeStats}>
                    <Text size="sm" fw={600}>
                      {rt.count.toLocaleString()}
                    </Text>
                    <Text size="xs" c="dimmed">
                      {pct.toFixed(1)}%
                    </Text>
                  </span>
                </div>
                <div className={classes.typeBar}>
                  <div className={classes.typeBarFill} style={{ width: `${barPct}%` }} />
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

export function PackageDetailPage() {
  const { name, version } = useParams<{ name: string; version: string }>();
  const navigate = useNavigate();
  const [activeTab, setActiveTab] = useState<string | null>("overview");
  const [typeFilter, setTypeFilter] = useState<string | null>(null);

  const { data, isLoading, error } = usePackageDetails(name || "", version || "");

  if (!name || !version) {
    return (
      <Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
        Invalid package parameters
      </Alert>
    );
  }

  return (
    <WorkspacePageLayout
      title={name}
      description={`Version ${version}`}
      kicker={
        <Breadcrumbs>
          <Anchor onClick={() => navigate("/packages")}>Packages</Anchor>
          <Text>{name}</Text>
        </Breadcrumbs>
      }
      actions={
        <div className={classes.headerActions}>
          <Button
            variant="subtle"
            leftSection={<ArrowLeft size={16} />}
            onClick={() => navigate("/packages")}
          >
            Back
          </Button>
          {data && (
            <FhirVersionBadge packageVersion={data.fhirVersion} isCompatible={data.isCompatible} />
          )}
        </div>
      }
      maxWidth={1280}
    >
      {isLoading && (
        <div className={classes.statePanel}>
          <Loader size="sm" />
          <Text size="sm" c="dimmed">
            Loading package details...
          </Text>
        </div>
      )}

      {error && (
        <Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
          {error instanceof Error ? error.message : "Failed to load package"}
        </Alert>
      )}

      {!isLoading && !error && data && (
        <Tabs value={activeTab} onChange={setActiveTab}>
          <Tabs.List>
            <Tabs.Tab value="overview">Overview</Tabs.Tab>
            <Tabs.Tab value="resources">
              Resources
              <Badge size="sm" variant="light" color="warm" ml="xs">
                {data.resourceCount}
              </Badge>
            </Tabs.Tab>
          </Tabs.List>

          <Tabs.Panel value="overview" pt="md">
            <PackageStats
              data={data}
              onSelectType={(type) => {
                setTypeFilter(type);
                setActiveTab("resources");
              }}
            />
          </Tabs.Panel>

          <Tabs.Panel value="resources" pt="md">
            <ResourcesTab
              packageName={name}
              packageVersion={version}
              resourceTypes={data.resourceTypes}
              filterType={typeFilter}
              onFilterTypeChange={setTypeFilter}
            />
          </Tabs.Panel>
        </Tabs>
      )}
    </WorkspacePageLayout>
  );
}

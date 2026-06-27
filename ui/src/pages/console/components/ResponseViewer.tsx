import {
  Badge,
  Button,
  DataPreview,
  DataTable,
  type DataTableColumn,
  IconCheck,
  IconX,
  Link,
  OperationOutcomePanel,
  SegmentedControl,
  Skeleton,
  Text,
} from "@octofhir/ui-kit";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  getBundleResourceEntries,
  getConsoleResponseStatusTone,
  isConsoleResponseError,
  isConsoleResponseSuccess,
  isFhirOperationOutcome,
  type RequestResponse,
} from "@/entities/rest-console-response";
import { JsonViewer } from "@/shared/ui-react/JsonViewer";
import { deriveBundleColumns, formatFhirValue } from "./fhirBundleTable";
import styles from "./ResponseViewer.module.css";

type ResourceEntry = {
  resource: Record<string, unknown> & { resourceType: string; id?: string };
  fullUrl?: string;
};

interface ResponseViewerProps {
  response?: RequestResponse;
  isLoading?: boolean;
}

function buildBundleColumns(
  entries: ResourceEntry[],
  onOpen: (resourceType: string, id: string) => void
): DataTableColumn<ResourceEntry>[] {
  const { columns: derived, mixedTypes } = deriveBundleColumns(entries.map((e) => e.resource));
  const columns: DataTableColumn<ResourceEntry>[] = [];
  if (mixedTypes) {
    columns.push({
      id: "__type",
      header: "Type",
      width: 150,
      accessor: (e) => e.resource.resourceType,
      cell: (e) => (
        <Badge size="sm" variant="light">
          {e.resource.resourceType}
        </Badge>
      ),
    });
  }
  columns.push({
    id: "__id",
    header: "ID",
    width: 200,
    sortable: true,
    filterable: true,
    accessor: (e) => e.resource.id ?? "",
    cell: (e) =>
      e.resource.id ? (
        <Link
          onClick={() => onOpen(e.resource.resourceType, e.resource.id as string)}
          className={styles.resourceLink}
        >
          {e.resource.id}
        </Link>
      ) : (
        <Text color="secondary">—</Text>
      ),
  });
  for (const col of derived) {
    columns.push({
      id: col.key,
      header: col.header,
      sortable: true,
      filterable: true,
      accessor: (e) => formatFhirValue(e.resource[col.key]),
      cell: (e) => <Text variant="body-2">{formatFhirValue(e.resource[col.key]) || "—"}</Text>,
    });
  }
  return columns;
}

export function ResponseViewer({ response, isLoading }: ResponseViewerProps) {
  const navigate = useNavigate();
  const [dataView, setDataView] = useState<"table" | "raw" | null>(null);
  const [showHeaders, setShowHeaders] = useState(false);

  if (isLoading) {
    return (
      <div className={styles.loading}>
        <Skeleton className={styles.skeletonHeader} />
        <Skeleton className={styles.skeletonBody} />
      </div>
    );
  }

  if (!response) {
    return (
      <div className={styles.empty}>
        <Text variant="body-2">No response data available.</Text>
      </div>
    );
  }

  const isSuccess = isConsoleResponseSuccess(response.status);
  const isError = isConsoleResponseError(response.status);
  const operationOutcome = isFhirOperationOutcome(response.body) ? response.body : null;
  const resourceEntries = getBundleResourceEntries(response.body);
  const hasResultEntries = resourceEntries.length > 0;
  const activeData = dataView ?? (hasResultEntries ? "table" : "raw");
  const dataOptions = hasResultEntries
    ? [
        { label: "Table", value: "table" },
        { label: "Raw", value: "raw" },
      ]
    : [{ label: "Raw", value: "raw" }];

  const handleOpenResource = (resourceType: string, resourceId: string) => {
    navigate(`/resources/${resourceType}/${resourceId}`);
  };

  const statusTheme = getConsoleResponseStatusTone(response.status);

  return (
    <div>
      {/* Status header */}
      <div className={styles.header}>
        <div className={styles.status}>
          <Badge theme={statusTheme} size="lg">
            <span className={styles.statusBadge}>
              {isSuccess ? <IconCheck size={14} /> : isError ? <IconX size={14} /> : null}
              {response.status} {response.statusText}
            </span>
          </Badge>
          <Text color="secondary" variant="caption-1">
            {response.durationMs}ms
          </Text>
        </div>

        <Text color="secondary" variant="caption-1">
          {new Date(response.requestedAt).toLocaleString()}
        </Text>
      </div>

      {/* OperationOutcome extraction */}
      {isError && operationOutcome && (
        <div className={styles.outcome}>
          <OperationOutcomePanel outcome={operationOutcome} title="FHIR error" maxIssues={4} />
        </div>
      )}

      {/* View controls: Table/Raw segment, with Headers as a separate toggle */}
      <div className={styles.viewBar}>
        <SegmentedControl
          size="sm"
          options={dataOptions}
          value={activeData}
          onChange={(value) => {
            setShowHeaders(false);
            setDataView(value === "table" || value === "raw" ? value : "raw");
          }}
        />
        <Button
          size="sm"
          variant={showHeaders ? "light" : "subtle"}
          onClick={() => setShowHeaders((s) => !s)}
        >
          Headers
        </Button>
      </div>

      <div className={styles.viewBody}>
        {showHeaders ? (
          response.headers ? (
            <DataPreview
              columns={[
                { id: "header", label: "Header", width: 260 },
                { id: "value", label: "Value" },
              ]}
              rows={Object.entries(response.headers).map(([key, value]) => ({
                header: (
                  <Text variant="body-2" className={styles.tableLabel}>
                    {key}
                  </Text>
                ),
                value: (
                  <Text color="secondary" variant="body-2">
                    {value}
                  </Text>
                ),
              }))}
              getRowKey={(_row, index) => Object.keys(response.headers ?? {})[index] ?? `${index}`}
            />
          ) : (
            <Text color="secondary">No headers</Text>
          )
        ) : activeData === "table" && hasResultEntries ? (
          <DataTable<ResourceEntry>
            data={resourceEntries as ResourceEntry[]}
            columns={buildBundleColumns(resourceEntries as ResourceEntry[], handleOpenResource)}
            getRowId={(e) => e.resource.id ?? e.fullUrl ?? e.resource.resourceType}
            size="sm"
            striped
            highlightOnHover
            stickyHeader
            paginated
            pageSize={25}
          />
        ) : response.body ? (
          <div className={styles.jsonFrame}>
            <JsonViewer data={response.body} maxHeight={600} />
          </div>
        ) : (
          <Text color="secondary">No response body</Text>
        )}
      </div>
    </div>
  );
}

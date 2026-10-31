import { Button, EmptyState, SegmentedRadioGroup, Skeleton } from "@octofhir/ui-kit";
import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { History, RotateCcw } from "lucide-react";
import { type KeyboardEvent, useMemo, useState } from "react";
import { useResourceHistory } from "@/shared/api/hooks";
import type { FhirResource } from "@/shared/api/types";
import { JsonDiffViewer } from "@/shared/monaco/JsonDiffViewer";
import { changedPaths, comparable, type ResourceVersion, toDiffText, toVersions } from "../lib/versions";
import classes from "./ResourceHistory.module.css";

dayjs.extend(relativeTime);

const ACTION_LABELS = { created: "Created", updated: "Updated", deleted: "Deleted" } as const;

/** Changed paths listed above the diff; the rest are summarised as "+N more". */
const MAX_PATHS = 8;

type Compare = "previous" | "current";

interface ResourceHistoryProps {
  resourceType: string;
  id: string;
  restoring: boolean;
  /** Saves an old version as the new current one. */
  onRestore: (version: ResourceVersion & { resource: FhirResource }) => void;
}

export function ResourceHistory({ resourceType, id, restoring, onRestore }: ResourceHistoryProps) {
  const { data, isLoading, isError, refetch } = useResourceHistory(resourceType, id);
  const versions = useMemo(() => toVersions(data), [data]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [compare, setCompare] = useState<Compare>("previous");

  const selectedIndex = Math.max(
    0,
    versions.findIndex((version) => version.versionId === selectedId)
  );
  const selected = versions[selectedIndex];
  const latest = versions[0];
  const base = compare === "previous" ? versions[selectedIndex + 1] : latest;
  const paths = useMemo(
    () => (selected ? changedPaths(comparable(base?.resource), comparable(selected.resource)) : []),
    [base, selected]
  );

  if (isLoading) {
    return (
      <div className={classes.loading}>
        <Skeleton className={classes.skeleton} />
      </div>
    );
  }
  if (isError || !selected) {
    return (
      <EmptyState
        image={<History width={32} height={32} aria-hidden="true" />}
        title={isError ? "History is unavailable" : "No history"}
        description={isError ? "The server did not return the versions of this resource." : undefined}
        actions={isError ? [{ text: "Retry", onClick: () => void refetch() }] : undefined}
      />
    );
  }

  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const next = versions[selectedIndex + (event.key === "ArrowDown" ? 1 : -1)];
    if (!next) return;
    setSelectedId(next.versionId);
    event.currentTarget.querySelector<HTMLElement>(`[data-version="${CSS.escape(next.versionId)}"]`)?.focus();
  };

  const isLatest = selected === latest;
  const sameAsBase = compare === "current" && isLatest;
  const restorable = !isLatest && selected.resource != null;

  return (
    <div className={classes.root}>
      <div className={classes.list} role="listbox" aria-label="Versions" onKeyDown={move}>
        {versions.map((version, index) => (
          <button
            key={version.versionId}
            type="button"
            role="option"
            aria-selected={version === selected}
            tabIndex={version === selected ? 0 : -1}
            data-version={version.versionId}
            className={classes.version}
            onClick={() => setSelectedId(version.versionId)}
          >
            <span className={classes.versionLine}>
              <span className={classes.versionId}>v{version.versionId}</span>
              {index === 0 && <span className={classes.current}>current</span>}
            </span>
            <span className={classes.versionMeta} data-action={version.action}>
              {ACTION_LABELS[version.action]}
              {version.at && (
                <span title={new Date(version.at).toLocaleString()}> · {dayjs(version.at).fromNow()}</span>
              )}
            </span>
          </button>
        ))}
      </div>

      <div className={classes.detail}>
        <div className={classes.detailHeader}>
          <SegmentedRadioGroup
            size="sm"
            aria-label="Compare with"
            value={compare}
            onChange={(value) => setCompare(value as Compare)}
            options={[
              { value: "previous", label: "vs previous" },
              { value: "current", label: "vs current" },
            ]}
          />
          {restorable && (
            <Button
              className={classes.restore}
              size="xs"
              variant="outline"
              loading={restoring}
              onClick={() => selected.resource && onRestore({ ...selected, resource: selected.resource })}
            >
              <Button.Icon side="left">
                <RotateCcw size={13} />
              </Button.Icon>
              Restore v{selected.versionId}
            </Button>
          )}
        </div>
        <div className={classes.paths}>
          {sameAsBase ? (
            <span className={classes.note}>This is the current version.</span>
          ) : !base && compare === "previous" ? (
            <span className={classes.note}>First version: everything is new.</span>
          ) : paths.length === 0 ? (
            <span className={classes.note}>No changes besides version metadata.</span>
          ) : (
            <>
              <span className={classes.note}>
                {paths.length} {paths.length === 1 ? "change" : "changes"}
              </span>
              {paths.slice(0, MAX_PATHS).map((path) => (
                <code key={path} className={classes.path}>
                  {path}
                </code>
              ))}
              {paths.length > MAX_PATHS && <span className={classes.note}>+{paths.length - MAX_PATHS} more</span>}
            </>
          )}
        </div>
        <div className={classes.diff}>
          <JsonDiffViewer original={toDiffText(base?.resource)} modified={toDiffText(selected.resource)} />
        </div>
      </div>
    </div>
  );
}

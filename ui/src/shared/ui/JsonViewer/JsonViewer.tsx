import { ActionIcon, Box, ScrollArea, TextInput, Tooltip } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconCopy, IconSearch } from "@tabler/icons-react";
import type React from "react";
import { useCallback, useMemo, useState } from "react";
import { formatJson, isValidJson } from "../../lib/json";
import styles from "./JsonViewer.module.css";

export interface JsonViewerProps {
  data: unknown;
  expanded?: boolean;
  maxHeight?: number;
  searchable?: boolean;
  copyable?: boolean;
  onError?: (error: Error) => void;
}

interface JsonNodeProps {
  value: unknown;
  path: string[];
  expanded: boolean;
  searchTerm: string;
  onToggle: (path: string[]) => void;
}

const JsonNode: React.FC<JsonNodeProps> = ({ value, path, expanded, searchTerm, onToggle }) => {
  const pathStr = path.join(".");
  const shouldHighlight = searchTerm && pathStr.toLowerCase().includes(searchTerm.toLowerCase());

  if (value === null) {
    return <span className={styles.null}>null</span>;
  }

  if (typeof value === "boolean") {
    return <span className={styles.boolean}>{value.toString()}</span>;
  }

  if (typeof value === "number") {
    return <span className={styles.number}>{value}</span>;
  }

  if (typeof value === "string") {
    const highlighted =
      shouldHighlight && searchTerm
        ? value.replace(new RegExp(`(${searchTerm})`, "gi"), "<mark>$1</mark>")
        : value;
    return (
      <span className={styles.string} dangerouslySetInnerHTML={{ __html: `"${highlighted}"` }} />
    );
  }

  if (Array.isArray(value)) {
    if (value.length === 0) {
      return <span className={styles.bracket}>[]</span>;
    }

    return (
      <div className={`${styles.collapsible} ${shouldHighlight ? styles.highlighted : ""}`}>
        <button
          type="button"
          className={styles.toggleButton}
          onClick={() => onToggle(path)}
          aria-label={`${expanded ? "Collapse" : "Expand"} array`}
        >
          <span className={styles.arrow}>{expanded ? "▼" : "▶"}</span>
          <span className={styles.bracket}>[</span>
          {!expanded && <span className={styles.preview}>{value.length} items</span>}
        </button>
        {expanded && (
          <div className={styles.content}>
            {value.map((item, index) => (
              <div key={index} className={styles.arrayItem}>
                <span className={styles.index}>{index}:</span>
                <JsonNode
                  value={item}
                  path={[...path, String(index)]}
                  expanded={true}
                  searchTerm={searchTerm}
                  onToggle={onToggle}
                />
                {index < value.length - 1 && <span className={styles.comma}>,</span>}
              </div>
            ))}
          </div>
        )}
        {expanded && <span className={styles.bracket}>]</span>}
      </div>
    );
  }

  if (typeof value === "object") {
    const entries = Object.entries(value);

    if (entries.length === 0) {
      return <span className={styles.brace}>{"{}"}</span>;
    }

    return (
      <div className={`${styles.collapsible} ${shouldHighlight ? styles.highlighted : ""}`}>
        <button
          type="button"
          className={styles.toggleButton}
          onClick={() => onToggle(path)}
          aria-label={`${expanded ? "Collapse" : "Expand"} object`}
        >
          <span className={styles.arrow}>{expanded ? "▼" : "▶"}</span>
          <span className={styles.brace}>{"{"}</span>
          {!expanded && <span className={styles.preview}>{entries.length} properties</span>}
        </button>
        {expanded && (
          <div className={styles.content}>
            {entries.map(([key, val], index) => (
              <div key={key} className={styles.objectProperty}>
                <span className={styles.key}>"{key}"</span>
                <span className={styles.colon}>:</span>
                <JsonNode
                  value={val}
                  path={[...path, key]}
                  expanded={true}
                  searchTerm={searchTerm}
                  onToggle={onToggle}
                />
                {index < entries.length - 1 && <span className={styles.comma}>,</span>}
              </div>
            ))}
          </div>
        )}
        {expanded && <span className={styles.brace}>{"}"}</span>}
      </div>
    );
  }

  return <span className={styles.unknown}>{String(value)}</span>;
};

export const JsonViewer: React.FC<JsonViewerProps> = ({
  data,
  expanded = true,
  maxHeight = 400,
  searchable = true,
  copyable = true,
  onError,
}) => {
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(new Set([""]));
  const [searchTerm, setSearchTerm] = useState("");

  const jsonString = useMemo(() => {
    try {
      return formatJson(data, { indent: 2 });
    } catch (error) {
      const err = error instanceof Error ? error : new Error("Failed to format JSON");
      onError?.(err);
      return String(data);
    }
  }, [data, onError]);

  const isValid = useMemo(() => {
    if (typeof data === "string") {
      return isValidJson(data);
    }
    return true;
  }, [data]);

  const handleToggle = useCallback((path: string[]) => {
    const pathStr = path.join(".");
    setExpandedPaths((prev) => {
      const newSet = new Set(prev);
      if (newSet.has(pathStr)) {
        newSet.delete(pathStr);
      } else {
        newSet.add(pathStr);
      }
      return newSet;
    });
  }, []);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(jsonString);
      notifications.show({
        title: "Copied to clipboard",
        message: "JSON data copied successfully",
        color: "green",
      });
    } catch (error) {
      notifications.show({
        title: "Copy failed",
        message: "Failed to copy JSON to clipboard",
        color: "red",
      });
    }
  }, [jsonString]);

  if (!isValid) {
    const error = new Error("Invalid JSON data provided");
    onError?.(error);
    return (
      <Box className={styles.error}>
        <div>Invalid JSON: {String(data)}</div>
      </Box>
    );
  }

  const isExpanded = (path: string[]): boolean => {
    return expanded || expandedPaths.has(path.join("."));
  };

  return (
    <Box className={styles.container}>
      {(searchable || copyable) && (
        <Box className={styles.toolbar}>
          {searchable && (
            <TextInput
              placeholder="Search JSON..."
              leftSection={<IconSearch size={16} />}
              value={searchTerm}
              onChange={(event) => setSearchTerm(event.currentTarget.value)}
              className={styles.searchInput}
            />
          )}
          {copyable && (
            <Tooltip label="Copy JSON">
              <ActionIcon variant="subtle" onClick={handleCopy}>
                <IconCopy size={16} />
              </ActionIcon>
            </Tooltip>
          )}
        </Box>
      )}

      <ScrollArea style={{ maxHeight }} className={styles.scrollArea}>
        <pre className={styles.jsonContent}>
          <JsonNode
            value={data}
            path={[]}
            expanded={isExpanded([])}
            searchTerm={searchTerm}
            onToggle={handleToggle}
          />
        </pre>
      </ScrollArea>
    </Box>
  );
};

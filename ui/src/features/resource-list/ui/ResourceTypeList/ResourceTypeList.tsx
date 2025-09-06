import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Loader,
  ScrollArea,
  Text,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { useDebouncedValue } from "@mantine/hooks";
import { IconRefresh, IconSearch } from "@tabler/icons-react";
import { useUnit } from "effector-react";
import type React from "react";
import { useCallback, useEffect, useState } from "react";
import { $selectedResourceType, setSelectedResourceType } from "@/entities/fhir";
import { serverApi } from "@/shared/api";
import styles from "./ResourceTypeList.module.css";

interface ResourceTypeListProps {
  className?: string;
  onResourceTypeSelect?: () => void;
}

interface ResourceType {
  name: string;
  count?: number;
}

export const ResourceTypeList: React.FC<ResourceTypeListProps> = ({
  className,
  onResourceTypeSelect,
}) => {
  const [resourceTypes, setResourceTypes] = useState<ResourceType[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState("");
  const [debouncedSearch] = useDebouncedValue(searchTerm, 300);

  const selectedResourceType = useUnit($selectedResourceType);

  // Load resource types from server
  const loadResourceTypes = useCallback(async () => {
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
  }, []);

  // Initial load
  useEffect(() => {
    loadResourceTypes();
  }, [loadResourceTypes]);

  // Filter resource types based on search
  const filteredResourceTypes = resourceTypes.filter((type) =>
    type.name.toLowerCase().includes(debouncedSearch.toLowerCase())
  );

  // Group resource types alphabetically
  const groupedResourceTypes = filteredResourceTypes.reduce(
    (acc, type) => {
      const firstLetter = type.name[0].toUpperCase();
      if (!acc[firstLetter]) {
        acc[firstLetter] = [];
      }
      acc[firstLetter].push(type);
      return acc;
    },
    {} as Record<string, ResourceType[]>
  );

  const handleResourceTypeSelect = useCallback(
    (resourceType: string) => {
      setSelectedResourceType(resourceType);
      onResourceTypeSelect?.();
    },
    [onResourceTypeSelect]
  );

  const handleRefresh = useCallback(() => {
    loadResourceTypes();
  }, [loadResourceTypes]);

  if (loading && resourceTypes.length === 0) {
    return (
      <Box className={`${styles.container} ${className || ""}`}>
        <Box className={styles.header}>
          <Text size="sm" fw={600}>
            Resource Types
          </Text>
        </Box>
        <Box className={styles.loadingContainer}>
          <Loader size="sm" />
          <Text size="sm" c="dimmed">
            Loading resource types...
          </Text>
        </Box>
      </Box>
    );
  }

  if (error && resourceTypes.length === 0) {
    return (
      <Box className={`${styles.container} ${className || ""}`}>
        <Box className={styles.header}>
          <Text size="sm" fw={600}>
            Resource Types
          </Text>
          <Tooltip label="Refresh">
            <ActionIcon size="sm" variant="subtle" onClick={handleRefresh}>
              <IconRefresh size={14} />
            </ActionIcon>
          </Tooltip>
        </Box>
        <Box className={styles.errorContainer}>
          <Text size="sm" c="red">
            {error}
          </Text>
          <Button size="xs" variant="outline" onClick={handleRefresh}>
            Retry
          </Button>
        </Box>
      </Box>
    );
  }

  return (
    <Box className={`${styles.container} ${className || ""}`}>
      <Box className={styles.header}>
        <Text size="sm" fw={600}>
          Resource Types
        </Text>
        <Box className={styles.headerActions}>
          {loading && <Loader size="xs" />}
          <Tooltip label="Refresh">
            <ActionIcon size="sm" variant="subtle" onClick={handleRefresh} loading={loading}>
              <IconRefresh size={14} />
            </ActionIcon>
          </Tooltip>
        </Box>
      </Box>

      <Box className={styles.searchContainer}>
        <TextInput
          placeholder="Search resource types..."
          leftSection={<IconSearch size={16} />}
          value={searchTerm}
          onChange={(event) => setSearchTerm(event.currentTarget.value)}
          size="sm"
        />
      </Box>

      <ScrollArea className={styles.listContainer}>
        {filteredResourceTypes.length === 0 ? (
          <Box className={styles.emptyState}>
            <Text size="sm" c="dimmed">
              {debouncedSearch ? "No matching resource types" : "No resource types available"}
            </Text>
          </Box>
        ) : (
          Object.keys(groupedResourceTypes)
            .sort()
            .map((letter) => (
              <Box key={letter} className={styles.group}>
                <Text size="xs" c="dimmed" fw={600} className={styles.groupHeader}>
                  {letter}
                </Text>
                {groupedResourceTypes[letter].map((type) => (
                  <Box
                    key={type.name}
                    className={`${styles.resourceType} ${
                      selectedResourceType === type.name ? styles.selected : ""
                    }`}
                    onClick={() => handleResourceTypeSelect(type.name)}
                  >
                    <Text size="sm">{type.name}</Text>
                    {type.count !== undefined && (
                      <Badge size="xs" color="gray" variant="light">
                        {type.count}
                      </Badge>
                    )}
                  </Box>
                ))}
              </Box>
            ))
        )}
      </ScrollArea>

      {error && resourceTypes.length > 0 && (
        <Box className={styles.errorBanner}>
          <Text size="xs" c="red">
            Failed to refresh: {error}
          </Text>
        </Box>
      )}
    </Box>
  );
};

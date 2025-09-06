import { ActionIcon, Group, Select, Text, Tooltip } from "@mantine/core";
import {
  IconChevronLeft,
  IconChevronRight,
  IconChevronsLeft,
  IconChevronsRight,
} from "@tabler/icons-react";
import { useUnit } from "effector-react";
import type React from "react";
import { useCallback } from "react";
import { $pagination, navigateToPageFx, setPageCount } from "@/entities/fhir";
import styles from "./Pagination.module.css";

interface PaginationProps {
  className?: string;
}

const PAGE_SIZE_OPTIONS = [
  { value: "10", label: "10 per page" },
  { value: "20", label: "20 per page" },
  { value: "50", label: "50 per page" },
  { value: "100", label: "100 per page" },
];

export const Pagination: React.FC<PaginationProps> = ({ className }) => {
  const pagination = useUnit($pagination);

  const handlePageSizeChange = useCallback((value: string | null) => {
    if (value) {
      setPageCount(Number(value));
    }
  }, []);

  const handleNavigation = useCallback((url: string) => {
    navigateToPageFx(url);
  }, []);

  const { links, count } = pagination;
  const hasAnyNavigation = links.first || links.prev || links.next || links.last;

  if (!hasAnyNavigation && !count) {
    return null;
  }

  return (
    <Group justify="space-between" className={`${styles.container} ${className || ""}`}>
      <Group gap="xs">
        <Select
          data={PAGE_SIZE_OPTIONS}
          value={String(count)}
          onChange={handlePageSizeChange}
          size="sm"
          w={140}
        />
      </Group>

      {hasAnyNavigation && (
        <Group gap="xs">
          <Tooltip label="First page">
            <ActionIcon
              variant="subtle"
              size="sm"
              disabled={!links.first}
              onClick={() => links.first && handleNavigation(links.first)}
            >
              <IconChevronsLeft size={16} />
            </ActionIcon>
          </Tooltip>

          <Tooltip label="Previous page">
            <ActionIcon
              variant="subtle"
              size="sm"
              disabled={!links.prev}
              onClick={() => links.prev && handleNavigation(links.prev)}
            >
              <IconChevronLeft size={16} />
            </ActionIcon>
          </Tooltip>

          <Text size="sm" c="dimmed" className={styles.pageInfo}>
            Page navigation
          </Text>

          <Tooltip label="Next page">
            <ActionIcon
              variant="subtle"
              size="sm"
              disabled={!links.next}
              onClick={() => links.next && handleNavigation(links.next)}
            >
              <IconChevronRight size={16} />
            </ActionIcon>
          </Tooltip>

          <Tooltip label="Last page">
            <ActionIcon
              variant="subtle"
              size="sm"
              disabled={!links.last}
              onClick={() => links.last && handleNavigation(links.last)}
            >
              <IconChevronsRight size={16} />
            </ActionIcon>
          </Tooltip>
        </Group>
      )}
    </Group>
  );
};

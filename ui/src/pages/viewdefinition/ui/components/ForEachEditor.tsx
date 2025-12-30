import { useCallback } from "react";
import {
  ActionIcon,
  Badge,
  Box,
  Card,
  Group,
  Stack,
  Text,
  Tooltip,
} from "@mantine/core";
import { IconArrowsExchange, IconPlus, IconTrash } from "@tabler/icons-react";
import { FHIRPathInput } from "./FHIRPathInput";
import { ColumnRow } from "./ColumnRow";
import type {
  ViewDefinitionColumn,
  ViewDefinitionConstant,
  ViewDefinitionSelect,
} from "../../lib/useViewDefinition";

interface ForEachEditorProps {
  selectNode: ViewDefinitionSelect;
  resourceType: string;
  constants?: ViewDefinitionConstant[];
  parentPath?: string[]; // FHIRPath context from parent forEach
  onChange: (node: ViewDefinitionSelect) => void;
  onRemove: () => void;
}

export function ForEachEditor({
  selectNode,
  resourceType,
  constants = [],
  parentPath = [],
  onChange,
  onRemove,
}: ForEachEditorProps) {
  const isForEachOrNull = !!selectNode.forEachOrNull;
  const forEachPath = selectNode.forEach || selectNode.forEachOrNull || "";
  const columns = selectNode.column || [];
  const nestedSelects = selectNode.select || [];

  // Build context path for nested FHIRPath autocomplete
  const contextPath = [...parentPath, forEachPath].filter(Boolean);

  const handlePathChange = useCallback(
    (path: string) => {
      const newNode = { ...selectNode };
      if (isForEachOrNull) {
        newNode.forEachOrNull = path;
        delete newNode.forEach;
      } else {
        newNode.forEach = path;
        delete newNode.forEachOrNull;
      }
      onChange(newNode);
    },
    [selectNode, isForEachOrNull, onChange]
  );

  const handleToggleType = useCallback(() => {
    const newNode = { ...selectNode };
    if (isForEachOrNull) {
      newNode.forEach = forEachPath;
      delete newNode.forEachOrNull;
    } else {
      newNode.forEachOrNull = forEachPath;
      delete newNode.forEach;
    }
    onChange(newNode);
  }, [selectNode, isForEachOrNull, forEachPath, onChange]);

  const handleColumnChange = useCallback(
    (index: number, column: ViewDefinitionColumn) => {
      const newColumns = [...columns];
      newColumns[index] = column;
      onChange({ ...selectNode, column: newColumns });
    },
    [selectNode, columns, onChange]
  );

  const handleColumnRemove = useCallback(
    (index: number) => {
      const newColumns = columns.filter((_, i) => i !== index);
      onChange({ ...selectNode, column: newColumns });
    },
    [selectNode, columns, onChange]
  );

  const handleColumnAdd = useCallback(() => {
    const newColumns = [
      ...columns,
      { name: "", path: "", _id: crypto.randomUUID() },
    ];
    onChange({ ...selectNode, column: newColumns });
  }, [selectNode, columns, onChange]);

  const handleNestedSelectChange = useCallback(
    (index: number, nested: ViewDefinitionSelect) => {
      const newSelects = [...nestedSelects];
      newSelects[index] = nested;
      onChange({ ...selectNode, select: newSelects });
    },
    [selectNode, nestedSelects, onChange]
  );

  const handleNestedSelectRemove = useCallback(
    (index: number) => {
      const newSelects = nestedSelects.filter((_, i) => i !== index);
      onChange({ ...selectNode, select: newSelects });
    },
    [selectNode, nestedSelects, onChange]
  );

  const handleAddNestedForEach = useCallback(() => {
    const newSelects = [
      ...nestedSelects,
      { forEach: "", column: [], _id: crypto.randomUUID() },
    ];
    onChange({ ...selectNode, select: newSelects });
  }, [selectNode, nestedSelects, onChange]);

  return (
    <Card withBorder padding="sm" radius="md">
      <Stack gap="sm">
        {/* Header */}
        <Group justify="space-between">
          <Group gap="xs">
            <Badge
              color={isForEachOrNull ? "orange" : "yellow"}
              variant="light"
              size="sm"
            >
              {isForEachOrNull ? "forEachOrNull" : "forEach"}
            </Badge>
            <Tooltip
              label={`Switch to ${isForEachOrNull ? "forEach" : "forEachOrNull"}`}
            >
              <ActionIcon
                variant="subtle"
                size="xs"
                onClick={handleToggleType}
              >
                <IconArrowsExchange size={12} />
              </ActionIcon>
            </Tooltip>
          </Group>
          <Tooltip label="Remove">
            <ActionIcon
              variant="subtle"
              color="red"
              size="sm"
              onClick={onRemove}
            >
              <IconTrash size={14} />
            </ActionIcon>
          </Tooltip>
        </Group>

        {/* Path input */}
        <Box>
          <Text size="xs" c="dimmed" mb={4}>
            Collection path
          </Text>
          <FHIRPathInput
            value={forEachPath}
            onChange={handlePathChange}
            resourceType={resourceType}
            constants={constants}
            forEachContext={parentPath}
            placeholder="e.g., name, telecom, contact"
            size="xs"
          />
        </Box>

        {/* Columns within this forEach */}
        {columns.length > 0 && (
          <Box>
            <Text size="xs" c="dimmed" mb={4}>
              Columns
            </Text>
            <Stack gap="xs">
              {columns.map((col, i) => (
                <ColumnRow
                  key={col._id || i}
                  column={col}
                  index={i}
                  resourceType={resourceType}
                  constants={constants}
                  forEachContext={contextPath}
                  onChange={handleColumnChange}
                  onRemove={handleColumnRemove}
                />
              ))}
            </Stack>
          </Box>
        )}

        {/* Nested forEach */}
        {nestedSelects.map((nested, i) => (
          <ForEachEditor
            key={nested._id || i}
            selectNode={nested}
            resourceType={resourceType}
            constants={constants}
            parentPath={contextPath}
            onChange={(n) => handleNestedSelectChange(i, n)}
            onRemove={() => handleNestedSelectRemove(i)}
          />
        ))}

        {/* Add buttons */}
        <Group gap="xs">
          <Tooltip label="Add column">
            <ActionIcon variant="light" size="sm" onClick={handleColumnAdd}>
              <IconPlus size={12} />
            </ActionIcon>
          </Tooltip>
          <Text size="xs" c="dimmed">
            Add column
          </Text>
          <Text size="xs" c="dimmed">
            |
          </Text>
          <Tooltip label="Add nested forEach">
            <ActionIcon
              variant="light"
              color="yellow"
              size="sm"
              onClick={handleAddNestedForEach}
            >
              <IconPlus size={12} />
            </ActionIcon>
          </Tooltip>
          <Text size="xs" c="dimmed">
            Add nested forEach
          </Text>
        </Group>
      </Stack>
    </Card>
  );
}

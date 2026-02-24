import { memo } from "react";
import { Group, TextInput, Select, ActionIcon, Tooltip, Box } from "@/shared/ui";
import { IconTrash, IconGripVertical } from "@tabler/icons-react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { FHIRPathInput } from "./FHIRPathInput";
import type { ViewDefinitionColumn, ViewDefinitionConstant } from "../../lib/useViewDefinition";

interface ColumnRowProps {
  column: ViewDefinitionColumn;
  index: number;
  resourceType: string;
  constants?: ViewDefinitionConstant[];
  forEachContext?: string[];
  onChange: (index: number, column: ViewDefinitionColumn) => void;
  onRemove: (index: number) => void;
}

export const ColumnRow = memo(function ColumnRow({
  column,
  index,
  resourceType,
  constants = [],
  forEachContext,
  onChange,
  onRemove,
}: ColumnRowProps) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: column._id || index.toString() });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  };

  return (
    <Box ref={setNodeRef} style={style}>
      <Group gap="xs" wrap="nowrap">
        <ActionIcon
          variant="subtle"
          size="sm"
          style={{ cursor: isDragging ? "grabbing" : "grab" }}
          {...attributes}
          {...listeners}
        >
          <IconGripVertical size={14} />
        </ActionIcon>
        <TextInput
          placeholder="Column name"
          value={column.name}
          onChange={(e) => onChange(index, { ...column, name: e.target.value })}
          style={{ flex: 1 }}
          size="xs"
        />
        <Box style={{ flex: 2 }}>
          <FHIRPathInput
            value={column.path}
            onChange={(path) => onChange(index, { ...column, path })}
            resourceType={resourceType}
            constants={constants}
            forEachContext={forEachContext}
            placeholder="FHIRPath expression"
            size="xs"
          />
        </Box>
        <Select
          placeholder="Type"
          value={column.type || null}
          onChange={(value) => onChange(index, { ...column, type: value || undefined })}
          data={[
            { value: "string", label: "String" },
            { value: "integer", label: "Integer" },
            { value: "decimal", label: "Decimal" },
            { value: "boolean", label: "Boolean" },
            { value: "dateTime", label: "DateTime" },
            { value: "date", label: "Date" },
          ]}
          clearable
          size="xs"
          style={{ width: 120 }}
        />
        <Tooltip label="Remove column">
          <ActionIcon
            variant="subtle"
            color="red"
            size="sm"
            onClick={() => onRemove(index)}
          >
            <IconTrash size={14} />
          </ActionIcon>
        </Tooltip>
      </Group>
    </Box>
  );
});

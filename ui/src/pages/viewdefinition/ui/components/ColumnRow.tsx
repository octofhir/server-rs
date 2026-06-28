import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  ActionIcon,
  Checkbox,
  IconGripVertical,
  IconTrash,
  Select,
  TextInput,
  Tooltip,
} from "@octofhir/ui-kit";
import { memo } from "react";
import type { ViewDefinitionColumn, ViewDefinitionConstant } from "../../lib/useViewDefinition";
import classes from "./ColumnRow.module.css";
import { FHIRPathInput } from "./FHIRPathInput";

interface ColumnRowProps {
  column: ViewDefinitionColumn;
  index: number;
  resourceType: string;
  constants?: ViewDefinitionConstant[];
  forEachContext?: string[];
  /** Sampled value for this column from the first preview row, if available. */
  sampleValue?: string;
  onChange: (index: number, column: ViewDefinitionColumn) => void;
  onRemove: (index: number) => void;
}

export const ColumnRow = memo(function ColumnRow({
  column,
  index,
  resourceType,
  constants = [],
  forEachContext,
  sampleValue,
  onChange,
  onRemove,
}: ColumnRowProps) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: column._id || index.toString(),
  });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  };

  return (
    <div ref={setNodeRef} style={style}>
      <div className={classes.row}>
        <ActionIcon
          variant="subtle"
          size="sm"
          className={classes.dragHandle}
          data-dragging={isDragging ? "true" : undefined}
          {...attributes}
          {...listeners}
        >
          <IconGripVertical size={14} />
        </ActionIcon>
        <TextInput
          placeholder="Column name"
          value={column.name}
          onChange={(value) => onChange(index, { ...column, name: value })}
          className={classes.nameInput}
          size="xs"
        />
        <div className={classes.pathInput}>
          <FHIRPathInput
            value={column.path}
            onChange={(path) => onChange(index, { ...column, path })}
            resourceType={resourceType}
            constants={constants}
            forEachContext={forEachContext}
            placeholder="FHIRPath expression"
            size="xs"
          />
          {sampleValue !== undefined && sampleValue !== "" && (
            <span className={classes.sampleValue} title={sampleValue}>
              → {sampleValue}
            </span>
          )}
        </div>
        <Tooltip label="Collection (allow multiple values)">
          <div className={classes.collectionToggle}>
            <Checkbox
              checked={column.collection ?? false}
              onChange={(checked) =>
                onChange(index, { ...column, collection: checked || undefined })
              }
              aria-label="Collection column"
            />
            <span className={classes.collectionLabel}>[ ]</span>
          </div>
        </Tooltip>
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
          className={classes.typeSelect}
        />
        <Tooltip label="Remove column">
          <ActionIcon variant="subtle" color="red" size="sm" onClick={() => onRemove(index)}>
            <IconTrash size={14} />
          </ActionIcon>
        </Tooltip>
      </div>
    </div>
  );
});

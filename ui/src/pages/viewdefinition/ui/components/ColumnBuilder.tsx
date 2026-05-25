import { Button, IconChevronDown, IconPlus, IconRepeat, Menu, Text } from "@octofhir/ui-kit";
import { useCallback } from "react";
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { ColumnRow } from "./ColumnRow";
import { ForEachEditor } from "./ForEachEditor";
import type {
  ViewDefinitionColumn,
  ViewDefinitionConstant,
  ViewDefinitionSelect,
} from "../../lib/useViewDefinition";
import classes from "./ColumnBuilder.module.css";

interface ColumnBuilderProps {
  columns: ViewDefinitionColumn[];
  nestedSelects: ViewDefinitionSelect[];
  resourceType: string;
  constants?: ViewDefinitionConstant[];
  onChange: (index: number, column: ViewDefinitionColumn) => void;
  onRemove: (index: number) => void;
  onAdd: () => void;
  onReorder: (fromIndex: number, toIndex: number) => void;
  onAddForEach: (isNull: boolean) => void;
  onNestedSelectChange: (index: number, select: ViewDefinitionSelect) => void;
  onNestedSelectRemove: (index: number) => void;
}

export function ColumnBuilder({
  columns,
  nestedSelects,
  resourceType,
  constants = [],
  onChange,
  onRemove,
  onAdd,
  onReorder,
  onAddForEach,
  onNestedSelectChange,
  onNestedSelectRemove,
}: ColumnBuilderProps) {
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8,
      },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    })
  );

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;

      if (over && active.id !== over.id) {
        const oldIndex = columns.findIndex(
          (c) => (c._id || columns.indexOf(c).toString()) === active.id
        );
        const newIndex = columns.findIndex(
          (c) => (c._id || columns.indexOf(c).toString()) === over.id
        );

        if (oldIndex !== -1 && newIndex !== -1) {
          onReorder(oldIndex, newIndex);
        }
      }
    },
    [columns, onReorder]
  );

  // Get sortable IDs for the columns
  const sortableIds = columns.map((col, i) => col._id || i.toString());

  return (
    <div className={classes.root}>
      <div className={classes.header}>
        <Text size="sm" fw={500}>
          Columns & Collections
        </Text>
        <div className={classes.actions}>
          <Button
            variant="subtle"
            size="xs"
            leftSection={<IconPlus size={12} />}
            onClick={onAdd}
          >
            Add Column
          </Button>
          <Menu shadow="md" width={180}>
            <Menu.Target>
              <Button
                variant="light"
                size="xs"
                leftSection={<IconRepeat size={12} />}
                rightSection={<IconChevronDown size={12} />}
              >
                Add Collection
              </Button>
            </Menu.Target>
            <Menu.Dropdown>
              <Menu.Item
                leftSection={<IconRepeat size={14} />}
                onClick={() => onAddForEach(false)}
              >
                forEach
              </Menu.Item>
              <Menu.Item
                leftSection={<IconRepeat size={14} />}
                onClick={() => onAddForEach(true)}
              >
                forEachOrNull
              </Menu.Item>
            </Menu.Dropdown>
          </Menu>
        </div>
      </div>

      <div className={classes.panel}>
        <div className={classes.content}>
          {/* Simple columns */}
          {columns.length > 0 && (
            <DndContext
              sensors={sensors}
              collisionDetection={closestCenter}
              onDragEnd={handleDragEnd}
            >
              <SortableContext
                items={sortableIds}
                strategy={verticalListSortingStrategy}
              >
                <div className={classes.columnList}>
                  {columns.map((col, i) => (
                    <ColumnRow
                      key={col._id || i}
                      column={col}
                      index={i}
                      resourceType={resourceType}
                      constants={constants}
                      onChange={onChange}
                      onRemove={onRemove}
                    />
                  ))}
                </div>
              </SortableContext>
            </DndContext>
          )}

          {/* Nested selects (forEach) */}
          {nestedSelects.map((select, i) => (
              <ForEachEditor
                key={select._id || i}
                selectNode={select}
                resourceType={resourceType}
                constants={constants}
                onChange={(s) => onNestedSelectChange(i, s)}
                onRemove={() => onNestedSelectRemove(i)}
              />
            )
          )}

          {/* Empty state */}
          {columns.length === 0 && nestedSelects.length === 0 && (
            <Text size="sm" c="dimmed" ta="center" py="md">
              No columns defined. Click "Add Column" or "Add Collection" to
              start.
            </Text>
          )}
        </div>
      </div>
    </div>
  );
}

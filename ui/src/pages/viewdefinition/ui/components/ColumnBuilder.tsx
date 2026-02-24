import { useCallback } from "react";
import { Button, Group, Menu, Paper, Stack, Text } from "@/shared/ui";
import { IconChevronDown, IconPlus, IconRepeat } from "@tabler/icons-react";
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
    <Stack gap="sm">
      <Group justify="space-between">
        <Text size="sm" fw={500}>
          Columns & Collections
        </Text>
        <Group gap="xs">
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
        </Group>
      </Group>

      <Paper withBorder p="sm">
        <Stack gap="md">
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
                <Stack gap="xs">
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
                </Stack>
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
        </Stack>
      </Paper>
    </Stack>
  );
}

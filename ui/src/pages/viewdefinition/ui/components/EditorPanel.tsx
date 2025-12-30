import { useCallback, useRef } from "react";
import { Group, Select, Stack, Tabs, TextInput } from "@mantine/core";
import { arrayMove } from "@dnd-kit/sortable";
import { ColumnBuilder } from "./ColumnBuilder";
import { ConstantsEditor } from "./ConstantsEditor";
import { WhereClauseEditor } from "./WhereClauseEditor";
import type {
  ViewDefinition,
  ViewDefinitionColumn,
  ViewDefinitionSelect,
} from "../../lib/useViewDefinition";

interface EditorPanelProps {
  viewDef: ViewDefinition;
  resourceTypes: string[];
  onChange: (viewDef: ViewDefinition) => void;
}

export function EditorPanel({ viewDef, resourceTypes, onChange }: EditorPanelProps) {
  const columns = viewDef.select[0]?.column || [];
  const nestedSelects = viewDef.select[0]?.select || [];

  // Use refs to keep callbacks stable (prevents child re-renders)
  const viewDefRef = useRef(viewDef);
  viewDefRef.current = viewDef;
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  const handleColumnChange = useCallback(
    (index: number, column: ViewDefinitionColumn) => {
      const current = viewDefRef.current;
      const newSelect = [...current.select];
      if (newSelect[0].column) {
        newSelect[0] = {
          ...newSelect[0],
          column: newSelect[0].column.map((c, i) => (i === index ? column : c)),
        };
      }
      onChangeRef.current({ ...current, select: newSelect });
    },
    []
  );

  const handleColumnRemove = useCallback(
    (index: number) => {
      const current = viewDefRef.current;
      const newSelect = [...current.select];
      if (newSelect[0].column) {
        newSelect[0] = {
          ...newSelect[0],
          column: newSelect[0].column.filter((_, i) => i !== index),
        };
      }
      onChangeRef.current({ ...current, select: newSelect });
    },
    []
  );

  const handleColumnAdd = useCallback(() => {
    const current = viewDefRef.current;
    const newSelect = [...current.select];
    if (!newSelect[0].column) {
      newSelect[0] = { ...newSelect[0], column: [] };
    }
    newSelect[0] = {
      ...newSelect[0],
      column: [...(newSelect[0].column || []), { name: "", path: "", _id: crypto.randomUUID() }],
    };
    onChangeRef.current({ ...current, select: newSelect });
  }, []);

  const handleColumnReorder = useCallback(
    (fromIndex: number, toIndex: number) => {
      const current = viewDefRef.current;
      const newSelect = [...current.select];
      if (newSelect[0].column) {
        newSelect[0] = {
          ...newSelect[0],
          column: arrayMove(newSelect[0].column, fromIndex, toIndex),
        };
      }
      onChangeRef.current({ ...current, select: newSelect });
    },
    []
  );

  const handleAddForEach = useCallback(
    (isNull: boolean) => {
      const current = viewDefRef.current;
      const newSelect = [...current.select];
      const newForEach: ViewDefinitionSelect = isNull
        ? { forEachOrNull: "", column: [], _id: crypto.randomUUID() }
        : { forEach: "", column: [], _id: crypto.randomUUID() };

      if (!newSelect[0].select) {
        newSelect[0] = { ...newSelect[0], select: [] };
      }
      newSelect[0] = {
        ...newSelect[0],
        select: [...(newSelect[0].select || []), newForEach],
      };
      onChangeRef.current({ ...current, select: newSelect });
    },
    []
  );

  const handleNestedSelectChange = useCallback(
    (index: number, nestedSelect: ViewDefinitionSelect) => {
      const current = viewDefRef.current;
      const newSelect = [...current.select];
      if (newSelect[0].select) {
        newSelect[0] = {
          ...newSelect[0],
          select: newSelect[0].select.map((s, i) => (i === index ? nestedSelect : s)),
        };
      }
      onChangeRef.current({ ...current, select: newSelect });
    },
    []
  );

  const handleNestedSelectRemove = useCallback(
    (index: number) => {
      const current = viewDefRef.current;
      const newSelect = [...current.select];
      if (newSelect[0].select) {
        newSelect[0] = {
          ...newSelect[0],
          select: newSelect[0].select.filter((_, i) => i !== index),
        };
      }
      onChangeRef.current({ ...current, select: newSelect });
    },
    []
  );

  return (
    <Stack gap="md" style={{ flex: 1 }}>
      {/* Basic info */}
      <Group grow>
        <TextInput
          label="Name"
          placeholder="my_patient_view"
          value={viewDef.name}
          onChange={(e) => onChange({ ...viewDef, name: e.target.value })}
          required
        />
        <Select
          label="Resource"
          placeholder="Select resource type"
          value={viewDef.resource}
          onChange={(value) => onChange({ ...viewDef, resource: value || "Patient" })}
          data={resourceTypes}
          searchable
        />
        <Select
          label="Status"
          value={viewDef.status}
          onChange={(value) =>
            onChange({ ...viewDef, status: (value as "draft" | "active") || "draft" })
          }
          data={[
            { value: "draft", label: "Draft" },
            { value: "active", label: "Active" },
            { value: "retired", label: "Retired" },
          ]}
        />
      </Group>

      {/* Tabs for different editors */}
      <Tabs defaultValue="columns" style={{ flex: 1 }}>
        <Tabs.List>
          <Tabs.Tab value="columns">Columns</Tabs.Tab>
          <Tabs.Tab value="where">Where</Tabs.Tab>
          <Tabs.Tab value="constants">Constants</Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="columns" pt="md">
          <ColumnBuilder
            columns={columns}
            nestedSelects={nestedSelects}
            resourceType={viewDef.resource}
            constants={viewDef.constant}
            onChange={handleColumnChange}
            onRemove={handleColumnRemove}
            onAdd={handleColumnAdd}
            onReorder={handleColumnReorder}
            onAddForEach={handleAddForEach}
            onNestedSelectChange={handleNestedSelectChange}
            onNestedSelectRemove={handleNestedSelectRemove}
          />
        </Tabs.Panel>

        <Tabs.Panel value="where" pt="md">
          <WhereClauseEditor
            whereClauses={viewDef.where || []}
            resourceType={viewDef.resource}
            constants={viewDef.constant}
            onChange={(where) => onChange({ ...viewDef, where })}
          />
        </Tabs.Panel>

        <Tabs.Panel value="constants" pt="md">
          <ConstantsEditor
            constants={viewDef.constant || []}
            onChange={(constant) => onChange({ ...viewDef, constant })}
          />
        </Tabs.Panel>
      </Tabs>
    </Stack>
  );
}

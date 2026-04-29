import { useCallback, useRef } from "react";
import { Flex, Select, Tabs, TextInput } from "@/shared/ui";
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
  value: ViewDefinition;
  resourceTypes: string[];
  onChange: (viewDef: ViewDefinition) => void;
}

export function EditorPanel({ value: viewDef, resourceTypes, onChange }: EditorPanelProps) {
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
          column: newSelect[0].column, // Keep columns
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
          column: newSelect[0].column, // Keep columns
          select: newSelect[0].select.filter((_, i) => i !== index),
        };
      }
      onChangeRef.current({ ...current, select: newSelect });
    },
    []
  );

  return (
    <Flex direction="column" gap="4" style={{ flex: 1 }}>
      {/* Basic info */}
      <Flex gap="4" alignItems="flex-end" wrap="wrap">
        <TextInput
          label="Name"
          placeholder="my_patient_view"
          value={viewDef.name}
          onChange={(e) => onChange({ ...viewDef, name: e.target.value })}
          style={{ flex: 1, minWidth: 200 }}
        />
        <Select
          label="Resource"
          placeholder="Select resource type"
          value={viewDef.resource}
          onUpdate={(value) => onChange({ ...viewDef, resource: value[0] || "Patient" })}
          data={resourceTypes}
          searchable
          style={{ flex: 1, minWidth: 200 }}
        />
        <Select
          label="Status"
          value={viewDef.status}
          onUpdate={(value) =>
            onChange({ ...viewDef, status: (value[0] as "draft" | "active") || "draft" })
          }
          data={[
            { value: "draft", label: "Draft" },
            { value: "active", label: "Active" },
            { value: "retired", label: "Retired" },
          ]}
          style={{ width: 120 }}
        />
      </Flex>

      {/* Tabs for different editors */}
      <Tabs defaultValue="columns" style={{ flex: 1, display: "flex", flexDirection: "column" }}>
        <Tabs.List>
          <Tabs.Tab value="columns">Columns</Tabs.Tab>
          <Tabs.Tab value="where">Where</Tabs.Tab>
          <Tabs.Tab value="constants">Constants</Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="columns" style={{ paddingTop: 16 }}>
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

        <Tabs.Panel value="where" style={{ paddingTop: 16 }}>
          <WhereClauseEditor
            whereClauses={viewDef.where || []}
            resourceType={viewDef.resource}
            constants={viewDef.constant}
            onChange={(where) => onChange({ ...viewDef, where })}
          />
        </Tabs.Panel>

        <Tabs.Panel value="constants" style={{ paddingTop: 16 }}>
          <ConstantsEditor
            constants={viewDef.constant || []}
            onChange={(constant) => onChange({ ...viewDef, constant })}
          />
        </Tabs.Panel>
      </Tabs>
    </Flex>
  );
}

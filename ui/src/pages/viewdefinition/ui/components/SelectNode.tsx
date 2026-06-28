import {
  closestCenter,
  DndContext,
  type DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  ActionIcon,
  Button,
  IconPlus,
  IconRepeat,
  IconTrash,
  Select,
  Text,
  Tooltip,
} from "@octofhir/ui-kit";
import { Layers, Rows3 } from "lucide-react";
import { useCallback } from "react";
import type {
  ViewDefinitionColumn,
  ViewDefinitionConstant,
  ViewDefinitionSelect,
} from "../../lib/useViewDefinition";
import { ColumnRow } from "./ColumnRow";
import { FHIRPathInput } from "./FHIRPathInput";
import classes from "./SelectNode.module.css";

type IterationKind = "none" | "forEach" | "forEachOrNull";

function iterationKind(node: ViewDefinitionSelect): IterationKind {
  if (node.forEach !== undefined) return "forEach";
  if (node.forEachOrNull !== undefined) return "forEachOrNull";
  return "none";
}

const uid = () => crypto.randomUUID();

interface SelectNodeProps {
  value: ViewDefinitionSelect;
  onChange: (next: ViewDefinitionSelect) => void;
  onRemove?: () => void;
  resourceType: string;
  constants?: ViewDefinitionConstant[];
  /** Accumulated forEach paths up the tree, for FHIRPath autocomplete context. */
  forEachContext?: string[];
  /** Map of column name → sampled value (from first preview row). */
  sampleRow?: Record<string, string>;
  depth: number;
  variant?: "root" | "select" | "union";
  label?: string;
}

/**
 * Recursive editor for a single `select` node of a SQL-on-FHIR ViewDefinition.
 * Handles its own `column[]`, an optional iteration (`forEach` /
 * `forEachOrNull`), nested `select[]`, and `unionAll[]` branches — each child
 * rendered by another `SelectNode`.
 */
export function SelectNode({
  value,
  onChange,
  onRemove,
  resourceType,
  constants = [],
  forEachContext = [],
  sampleRow,
  depth,
  variant = "select",
  label,
}: SelectNodeProps) {
  const kind = iterationKind(value);
  const iterationPath = value.forEach ?? value.forEachOrNull ?? "";
  const childContext =
    kind !== "none" && iterationPath ? [...forEachContext, iterationPath] : forEachContext;

  const columns = value.column ?? [];
  const nested = value.select ?? [];
  const unions = value.unionAll ?? [];

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );

  // --- column handlers ---
  const setColumns = useCallback(
    (next: ViewDefinitionColumn[]) => onChange({ ...value, column: next }),
    [onChange, value]
  );

  const handleColumnChange = useCallback(
    (index: number, column: ViewDefinitionColumn) =>
      setColumns(columns.map((c, i) => (i === index ? column : c))),
    [columns, setColumns]
  );
  const handleColumnRemove = useCallback(
    (index: number) => setColumns(columns.filter((_, i) => i !== index)),
    [columns, setColumns]
  );
  const handleColumnAdd = useCallback(
    () => setColumns([...columns, { name: "", path: "", _id: uid() }]),
    [columns, setColumns]
  );
  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const ids = columns.map((c, i) => c._id || i.toString());
      const from = ids.indexOf(String(active.id));
      const to = ids.indexOf(String(over.id));
      if (from !== -1 && to !== -1) setColumns(arrayMove(columns, from, to));
    },
    [columns, setColumns]
  );

  // --- iteration handlers ---
  const handleIterationChange = useCallback(
    (next: IterationKind) => {
      const path = iterationPath;
      const base = { ...value };
      delete base.forEach;
      delete base.forEachOrNull;
      if (next === "forEach") base.forEach = path;
      else if (next === "forEachOrNull") base.forEachOrNull = path;
      onChange(base);
    },
    [iterationPath, onChange, value]
  );
  const handleIterationPath = useCallback(
    (path: string) => {
      if (kind === "forEach") onChange({ ...value, forEach: path });
      else if (kind === "forEachOrNull") onChange({ ...value, forEachOrNull: path });
    },
    [kind, onChange, value]
  );

  // --- nested select / union handlers ---
  const addNested = useCallback(
    () =>
      onChange({
        ...value,
        select: [
          ...nested,
          { forEach: "", column: [{ name: "", path: "", _id: uid() }], _id: uid() },
        ],
      }),
    [nested, onChange, value]
  );
  const changeNested = useCallback(
    (index: number, node: ViewDefinitionSelect) =>
      onChange({ ...value, select: nested.map((s, i) => (i === index ? node : s)) }),
    [nested, onChange, value]
  );
  const removeNested = useCallback(
    (index: number) => onChange({ ...value, select: nested.filter((_, i) => i !== index) }),
    [nested, onChange, value]
  );

  const addUnion = useCallback(
    () =>
      onChange({
        ...value,
        unionAll: [...unions, { column: [{ name: "", path: "", _id: uid() }], _id: uid() }],
      }),
    [unions, onChange, value]
  );
  const changeUnion = useCallback(
    (index: number, node: ViewDefinitionSelect) =>
      onChange({ ...value, unionAll: unions.map((s, i) => (i === index ? node : s)) }),
    [unions, onChange, value]
  );
  const removeUnion = useCallback(
    (index: number) => onChange({ ...value, unionAll: unions.filter((_, i) => i !== index) }),
    [unions, onChange, value]
  );

  const sortableIds = columns.map((c, i) => c._id || i.toString());

  return (
    <div className={classes.node} data-variant={variant} data-depth={depth}>
      <div className={classes.nodeHeader}>
        <span className={classes.nodeBadge}>
          {variant === "union" ? <Layers size={13} /> : <Rows3 size={13} />}
          <Text size="xs" fw={600}>
            {label ?? "Select"}
          </Text>
        </span>

          <Select
            size="xs"
            value={kind}
            onChange={(v) => handleIterationChange((v as IterationKind) || "none")}
            data={[
              { value: "none", label: "Plain rows" },
              { value: "forEach", label: "forEach" },
              { value: "forEachOrNull", label: "forEachOrNull" },
            ]}
            className={classes.iterationSelect}
          />

          {kind !== "none" && (
            <div className={classes.iterationPath}>
              <Text size="xs" c="dimmed" className={classes.iterationLabel}>
                iterates over
              </Text>
              <div className={classes.iterationField}>
                <FHIRPathInput
                  value={iterationPath}
                  onChange={handleIterationPath}
                  resourceType={resourceType}
                  constants={constants}
                  forEachContext={forEachContext}
                  placeholder="e.g. name"
                  size="xs"
                />
              </div>
            </div>
          )}

          {onRemove && (
            <Tooltip label="Remove group">
              <ActionIcon variant="subtle" color="red" size="sm" onClick={onRemove}>
                <IconTrash size={14} />
              </ActionIcon>
            </Tooltip>
          )}
        </div>

      <div className={classes.nodeBody}>
        {/* Columns */}
        {columns.length > 0 && (
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={handleDragEnd}
          >
            <div className={classes.columnHead}>
              <span />
              <span>Name</span>
              <span>FHIRPath</span>
              <span className={classes.colHeadCenter}>[ ]</span>
              <span>Type</span>
              <span />
            </div>
            <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
              <div className={classes.columnList}>
                {columns.map((col, i) => (
                  <ColumnRow
                    key={col._id || i}
                    column={col}
                    index={i}
                    resourceType={resourceType}
                    constants={constants}
                    forEachContext={childContext}
                    sampleValue={col.name ? sampleRow?.[col.name] : undefined}
                    onChange={handleColumnChange}
                    onRemove={handleColumnRemove}
                  />
                ))}
              </div>
            </SortableContext>
          </DndContext>
        )}

        {/* Nested selects */}
        {nested.map((node, i) => (
          <SelectNode
            key={node._id || i}
            value={node}
            onChange={(n) => changeNested(i, n)}
            onRemove={() => removeNested(i)}
            resourceType={resourceType}
            constants={constants}
            forEachContext={childContext}
            sampleRow={sampleRow}
            depth={depth + 1}
            variant="select"
            label="Nested select"
          />
        ))}

        {/* Union branches */}
        {unions.map((node, i) => (
          <SelectNode
            key={node._id || i}
            value={node}
            onChange={(n) => changeUnion(i, n)}
            onRemove={() => removeUnion(i)}
            resourceType={resourceType}
            constants={constants}
            forEachContext={childContext}
            sampleRow={sampleRow}
            depth={depth + 1}
            variant="union"
            label={`Union branch ${i + 1}`}
          />
        ))}

        {columns.length === 0 && nested.length === 0 && unions.length === 0 && (
          <Text size="xs" c="dimmed" className={classes.emptyHint}>
            Empty select — add a column, a for-each group, or a union branch.
          </Text>
        )}

        {/* Affordances */}
        <div className={classes.affordances}>
          <Button
            variant="subtle"
            size="xs"
            leftSection={<IconPlus size={12} />}
            onClick={handleColumnAdd}
          >
            Column
          </Button>
          <Button
            variant="subtle"
            size="xs"
            leftSection={<IconRepeat size={12} />}
            onClick={addNested}
          >
            For-each group
          </Button>
          <Button variant="subtle" size="xs" leftSection={<Layers size={12} />} onClick={addUnion}>
            Union branch
          </Button>
        </div>
      </div>
    </div>
  );
}

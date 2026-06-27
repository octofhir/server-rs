import { ActionIcon, Badge, Button, Drawer, Text, TextInput } from "@octofhir/ui-kit";
import { Check, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useEnvironments } from "../hooks/useEnvironments";
import type { EnvVariable } from "../services/environmentService";
import styles from "./EnvironmentPanel.module.css";

interface Props {
  opened: boolean;
  onClose: () => void;
}

export function EnvironmentPanel({ opened, onClose }: Props) {
  const {
    environments,
    active,
    activeId,
    setActive,
    createEnvironment,
    updateEnvironment,
    removeEnvironment,
  } = useEnvironments();

  const [newName, setNewName] = useState("");
  const [vars, setVars] = useState<EnvVariable[]>([]);
  const [dirty, setDirty] = useState(false);

  // Load the active env's variables into the editor when it changes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: sync on active env identity
  useEffect(() => {
    setVars(active ? active.variables.map((v) => ({ ...v })) : []);
    setDirty(false);
  }, [active?.id]);

  const updateVar = (i: number, patch: Partial<EnvVariable>) => {
    setVars((prev) => prev.map((v, idx) => (idx === i ? { ...v, ...patch } : v)));
    setDirty(true);
  };
  const addVar = () => {
    setVars((prev) => [...prev, { key: "", value: "" }]);
    setDirty(true);
  };
  const removeVar = (i: number) => {
    setVars((prev) => prev.filter((_, idx) => idx !== i));
    setDirty(true);
  };

  const save = async () => {
    if (!active) return;
    await updateEnvironment({ ...active, variables: vars.filter((v) => v.key.trim()) });
    setDirty(false);
  };

  return (
    <Drawer
      open={opened}
      onOpenChange={(next) => !next && onClose()}
      placement="right"
      size={460}
      title="Environments"
    >
      <div className={styles.root}>
        {/* Environment selector */}
        <div className={styles.envBar}>
          <button
            type="button"
            className={styles.envChip}
            data-active={activeId === null ? "1" : undefined}
            onClick={() => setActive(null)}
          >
            No env
          </button>
          {environments.map((e) => (
            <button
              key={e.id}
              type="button"
              className={styles.envChip}
              data-active={e.id === activeId ? "1" : undefined}
              onClick={() => setActive(e.id)}
            >
              {e.name}
            </button>
          ))}
        </div>

        <div className={styles.newRow}>
          <TextInput placeholder="New environment name" value={newName} onChange={setNewName} />
          <Button
            size="sm"
            variant="light"
            disabled={!newName.trim()}
            onClick={async () => {
              await createEnvironment(newName.trim());
              setNewName("");
            }}
          >
            <Button.Icon>
              <Plus size={14} />
            </Button.Icon>
            Add
          </Button>
        </div>

        {active ? (
          <div className={styles.editor}>
            <div className={styles.editorHead}>
              <Text className={styles.sectionLabel}>{active.name} variables</Text>
              <ActionIcon
                size="sm"
                variant="subtle"
                aria-label="Delete environment"
                onClick={() => removeEnvironment(active.id)}
              >
                <Trash2 size={14} />
              </ActionIcon>
            </div>

            <div className={styles.varList}>
              {vars.length === 0 ? (
                <Text size="sm" c="dimmed">
                  No variables. Add one, then use it as <code>{"{{key}}"}</code> in any request.
                </Text>
              ) : (
                vars.map((v, i) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: row identity is positional
                  <div key={i} className={styles.varRow}>
                    <TextInput
                      placeholder="key"
                      value={v.key}
                      onChange={(val) => updateVar(i, { key: val })}
                    />
                    <TextInput
                      placeholder="value"
                      value={v.value}
                      onChange={(val) => updateVar(i, { value: val })}
                    />
                    <ActionIcon
                      size="sm"
                      variant="subtle"
                      aria-label="Remove variable"
                      onClick={() => removeVar(i)}
                    >
                      <Trash2 size={14} />
                    </ActionIcon>
                  </div>
                ))
              )}
            </div>

            <div className={styles.editorActions}>
              <Button size="sm" variant="subtle" onClick={addVar}>
                <Button.Icon>
                  <Plus size={14} />
                </Button.Icon>
                Variable
              </Button>
              <Button size="sm" variant="filled" disabled={!dirty} onClick={save}>
                <Button.Icon>
                  <Check size={14} />
                </Button.Icon>
                Save
              </Button>
            </div>
          </div>
        ) : (
          <div className={styles.empty}>
            <Badge size="sm" variant="light">
              {"{{var}}"}
            </Badge>
            <Text size="sm" c="dimmed">
              Select or create an environment to define variables. Use <code>{"{{key}}"}</code> in
              the URL, headers or body — plus dynamics like <code>{"{{$guid}}"}</code>,{" "}
              <code>{"{{$now}}"}</code>.
            </Text>
          </div>
        )}
      </div>
    </Drawer>
  );
}

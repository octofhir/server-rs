import { ActionIcon, Button, IconPlus, IconTrash, Select, Text, TextInput, Tooltip } from "@octofhir/ui-kit";
import type { ViewDefinitionConstant } from "../../lib/useViewDefinition";
import classes from "./ConstantsEditor.module.css";

interface ConstantsEditorProps {
  constants: ViewDefinitionConstant[];
  onChange: (constants: ViewDefinitionConstant[]) => void;
}

type ConstantValueType = "string" | "integer" | "decimal" | "boolean";

const VALUE_TYPE_OPTIONS: Array<{ value: ConstantValueType; label: string }> = [
  { value: "string", label: "String" },
  { value: "integer", label: "Integer" },
  { value: "decimal", label: "Decimal" },
  { value: "boolean", label: "Boolean" },
];

function getValueType(constant: ViewDefinitionConstant): ConstantValueType {
  if (constant.valueInteger !== undefined) return "integer";
  if (constant.valueDecimal !== undefined) return "decimal";
  if (constant.valueBoolean !== undefined) return "boolean";
  return "string";
}

function getValue(constant: ViewDefinitionConstant, valueType: ConstantValueType) {
  switch (valueType) {
    case "integer":
      return constant.valueInteger ?? 0;
    case "decimal":
      return constant.valueDecimal ?? 0;
    case "boolean":
      return constant.valueBoolean ?? false;
    case "string":
      return constant.valueString ?? "";
  }
}

function valueFromInput(valueType: ConstantValueType, value: string): string | number | boolean {
  switch (valueType) {
    case "integer":
      return Number.parseInt(value, 10) || 0;
    case "decimal":
      return Number.parseFloat(value) || 0;
    case "boolean":
      return value === "true";
    case "string":
      return value;
  }
}

function withConstantName(constant: ViewDefinitionConstant, name: string): ViewDefinitionConstant {
  return { ...constant, name };
}

function withConstantValue(
  constant: ViewDefinitionConstant,
  valueType: ConstantValueType,
  value: string | number | boolean,
): ViewDefinitionConstant {
  const next: ViewDefinitionConstant = {
    name: constant.name,
    _id: constant._id,
  };

  switch (valueType) {
    case "integer":
      next.valueInteger = typeof value === "number" ? Math.trunc(value) : 0;
      break;
    case "decimal":
      next.valueDecimal = typeof value === "number" ? value : 0;
      break;
    case "boolean":
      next.valueBoolean = typeof value === "boolean" ? value : false;
      break;
    case "string":
      next.valueString = typeof value === "string" ? value : String(value);
      break;
  }

  return next;
}

function isConstantValueType(value: string | null): value is ConstantValueType {
  return VALUE_TYPE_OPTIONS.some((option) => option.value === value);
}

export function ConstantsEditor({ constants, onChange }: ConstantsEditorProps) {
  const handleAdd = () => {
    onChange([...constants, { name: "", valueString: "", _id: crypto.randomUUID() }]);
  };

  const handleNameChange = (index: number, name: string) => {
    const updated = constants.map((constant, i) => {
      if (i !== index) return constant;
      return withConstantName(constant, name);
    });
    onChange(updated);
  };

  const handleValueChange = (
    index: number,
    valueType: ConstantValueType,
    value: string | number | boolean,
  ) => {
    const updated = constants.map((constant, i) =>
      i === index ? withConstantValue(constant, valueType, value) : constant,
    );
    onChange(updated);
  };

  const handleRemove = (index: number) => {
    onChange(constants.filter((_, i) => i !== index));
  };

  return (
    <div className={classes.root}>
      <div className={classes.header}>
        <Text size="sm" fw={500}>
          Constants
        </Text>
        <Button
          variant="subtle"
          size="xs"
          leftSection={<IconPlus size={12} />}
          onClick={handleAdd}
        >
          Add Constant
        </Button>
      </div>
      <div className={classes.panel}>
        <div className={classes.list}>
          {constants.map((constant, i) => {
            const valueType = getValueType(constant);
            const value = getValue(constant, valueType);

            return (
              <div key={constant._id || `const-${i}`} className={classes.row}>
                <TextInput
                  placeholder="Name"
                  value={constant.name}
                  onChange={(value) => handleNameChange(i, value)}
                  className={classes.input}
                  size="xs"
                />
                <Select
                  value={valueType}
                  onChange={(type) => {
                    if (isConstantValueType(type)) {
                      handleValueChange(i, type, type === "boolean" ? false : type === "integer" || type === "decimal" ? 0 : "");
                    }
                  }}
                  data={VALUE_TYPE_OPTIONS}
                  size="xs"
                  className={classes.typeSelect}
                />
                <TextInput
                  placeholder="Value"
                  value={String(value || "")}
                  onChange={(value) => {
                    handleValueChange(i, valueType, valueFromInput(valueType, value));
                  }}
                  className={classes.input}
                  size="xs"
                />
                <Tooltip label="Remove constant">
                  <ActionIcon
                    variant="subtle"
                    color="red"
                    size="sm"
                    onClick={() => handleRemove(i)}
                  >
                    <IconTrash size={14} />
                  </ActionIcon>
                </Tooltip>
              </div>
            );
          })}
          {constants.length === 0 && (
            <Text size="sm" c="dimmed" ta="center" py="md">
              No constants defined
            </Text>
          )}
        </div>
      </div>
    </div>
  );
}

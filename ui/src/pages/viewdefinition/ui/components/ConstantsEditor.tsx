import { Stack, Button, Paper, Group, Text, TextInput, Select, ActionIcon, Tooltip } from "@mantine/core";
import { IconPlus, IconTrash } from "@tabler/icons-react";
import type { ViewDefinitionConstant } from "../../lib/useViewDefinition";

interface ConstantsEditorProps {
  constants: ViewDefinitionConstant[];
  onChange: (constants: ViewDefinitionConstant[]) => void;
}

export function ConstantsEditor({ constants, onChange }: ConstantsEditorProps) {
  const handleAdd = () => {
    onChange([...constants, { name: "", valueString: "", _id: crypto.randomUUID() }]);
  };

  const handleChange = (index: number, field: keyof ViewDefinitionConstant, value: string | number | boolean) => {
    const updated = constants.map((constant, i) => {
      if (i !== index) return constant;

      // Clear other value fields when type changes
      const newConstant: ViewDefinitionConstant = { name: constant.name };
      if (field === "name") {
        newConstant.name = value as string;
        // Preserve existing value
        if (constant.valueString !== undefined) newConstant.valueString = constant.valueString;
        if (constant.valueInteger !== undefined) newConstant.valueInteger = constant.valueInteger;
        if (constant.valueBoolean !== undefined) newConstant.valueBoolean = constant.valueBoolean;
        if (constant.valueDecimal !== undefined) newConstant.valueDecimal = constant.valueDecimal;
      } else {
        newConstant[field] = value as never;
      }

      return newConstant;
    });
    onChange(updated);
  };

  const handleRemove = (index: number) => {
    onChange(constants.filter((_, i) => i !== index));
  };

  return (
    <Stack gap="sm">
      <Group justify="space-between">
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
      </Group>
      <Paper withBorder p="sm">
        <Stack gap="xs">
          {constants.map((constant, i) => {
            const valueType =
              constant.valueString !== undefined ? "string" :
              constant.valueInteger !== undefined ? "integer" :
              constant.valueDecimal !== undefined ? "decimal" :
              constant.valueBoolean !== undefined ? "boolean" :
              "string";

            const value = constant[`value${valueType.charAt(0).toUpperCase()}${valueType.slice(1)}` as keyof ViewDefinitionConstant] as string | number | boolean;

            return (
              <Group key={constant._id || `const-${i}`} gap="xs" wrap="nowrap">
                <TextInput
                  placeholder="Name"
                  value={constant.name}
                  onChange={(e) => handleChange(i, "name", e.target.value)}
                  style={{ flex: 1 }}
                  size="xs"
                />
                <Select
                  value={valueType}
                  onChange={(type) => {
                    if (type) {
                      const field = `value${type.charAt(0).toUpperCase()}${type.slice(1)}` as keyof ViewDefinitionConstant;
                      handleChange(i, field, type === "boolean" ? false : type === "integer" || type === "decimal" ? 0 : "");
                    }
                  }}
                  data={[
                    { value: "string", label: "String" },
                    { value: "integer", label: "Integer" },
                    { value: "decimal", label: "Decimal" },
                    { value: "boolean", label: "Boolean" },
                  ]}
                  size="xs"
                  style={{ width: 100 }}
                />
                <TextInput
                  placeholder="Value"
                  value={String(value || "")}
                  onChange={(e) => {
                    const field = `value${valueType.charAt(0).toUpperCase()}${valueType.slice(1)}` as keyof ViewDefinitionConstant;
                    const newValue = valueType === "integer" ? parseInt(e.target.value) || 0 :
                                     valueType === "decimal" ? parseFloat(e.target.value) || 0 :
                                     valueType === "boolean" ? e.target.value === "true" :
                                     e.target.value;
                    handleChange(i, field, newValue);
                  }}
                  style={{ flex: 1 }}
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
              </Group>
            );
          })}
          {constants.length === 0 && (
            <Text size="sm" c="dimmed" ta="center" py="md">
              No constants defined
            </Text>
          )}
        </Stack>
      </Paper>
    </Stack>
  );
}

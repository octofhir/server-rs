import { Stack, Button, Paper, Group, Text, ActionIcon, Tooltip, Box } from "@/shared/ui";
import { IconPlus, IconTrash } from "@tabler/icons-react";
import type { ViewDefinitionWhere, ViewDefinitionConstant } from "../../lib/useViewDefinition";
import { FHIRPathInput } from "./FHIRPathInput";

interface WhereClauseEditorProps {
  whereClauses: ViewDefinitionWhere[];
  resourceType: string;
  constants?: ViewDefinitionConstant[];
  onChange: (clauses: ViewDefinitionWhere[]) => void;
}

export function WhereClauseEditor({
  whereClauses,
  resourceType,
  constants,
  onChange,
}: WhereClauseEditorProps) {
  const handleAdd = () => {
    onChange([...whereClauses, { path: "", _id: crypto.randomUUID() }]);
  };

  const handleChange = (index: number, path: string) => {
    const updated = whereClauses.map((clause, i) =>
      i === index ? { ...clause, path } : clause
    );
    onChange(updated);
  };

  const handleRemove = (index: number) => {
    onChange(whereClauses.filter((_, i) => i !== index));
  };

  return (
    <Stack gap="sm">
      <Group justify="space-between">
        <Text size="sm" fw={500}>
          Where Clauses
        </Text>
        <Button
          variant="subtle"
          size="xs"
          leftSection={<IconPlus size={12} />}
          onClick={handleAdd}
        >
          Add Where Clause
        </Button>
      </Group>
      <Paper withBorder p="sm">
        <Stack gap="xs">
          {whereClauses.map((clause, i) => (
            <Group key={clause._id || `where-${i}`} gap="xs" wrap="nowrap" align="center">
              <Box style={{ flex: 1 }}>
                <FHIRPathInput
                  value={clause.path}
                  onChange={(value) => handleChange(i, value)}
                  resourceType={resourceType}
                  constants={constants}
                  placeholder="FHIRPath expression (e.g., status = 'active')"
                  size="xs"
                />
              </Box>
              <Tooltip label="Remove clause">
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
          ))}
          {whereClauses.length === 0 && (
            <Text size="sm" c="dimmed" ta="center" py="md">
              No where clauses defined
            </Text>
          )}
        </Stack>
      </Paper>
    </Stack>
  );
}

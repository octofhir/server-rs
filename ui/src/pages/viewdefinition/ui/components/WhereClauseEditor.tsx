import { Button, Text, ActionIcon, Tooltip, Box } from "@/shared/ui";
import { IconPlus, IconTrash } from "@octofhir/ui-kit";
import type { ViewDefinitionWhere, ViewDefinitionConstant } from "../../lib/useViewDefinition";
import { FHIRPathInput } from "./FHIRPathInput";
import classes from "./WhereClauseEditor.module.css";

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
    <div className={classes.root}>
      <div className={classes.header}>
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
      </div>
      <div className={classes.panel}>
        <div className={classes.list}>
          {whereClauses.map((clause, i) => (
            <div key={clause._id || `where-${i}`} className={classes.row}>
              <Box className={classes.inputCell}>
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
            </div>
          ))}
          {whereClauses.length === 0 && (
            <Text size="sm" c="dimmed" ta="center" py="md">
              No where clauses defined
            </Text>
          )}
        </div>
      </div>
    </div>
  );
}

import {
  Stack,
  Select,
  Switch,
  NumberInput,
  Text,
  Divider,
  Group,
  Collapse,
  ActionIcon,
} from '@/shared/ui';
import { IconChevronDown, IconChevronRight } from '@tabler/icons-react';
import { useState } from 'react';
import {
  type FormatterConfig,
  type FormatterStyle,
  type SqlStyleConfig,
  type PgFormatterConfig,
  type KeywordCase,
  type IdentifierCase,
  type CommaStyle,
  type PgCaseOption,
  FORMATTER_STYLE_LABELS,
  KEYWORD_CASE_LABELS,
  IDENTIFIER_CASE_LABELS,
  COMMA_STYLE_LABELS,
  PG_CASE_OPTION_LABELS,
  getDefaultConfigForStyle,
} from './formatterTypes';

interface FormatterSettingsProps {
  value: FormatterConfig;
  onChange: (config: FormatterConfig) => void;
  /** Compact mode for toolbar display */
  compact?: boolean;
}

/**
 * Formatter settings component for configuring SQL formatting options.
 *
 * Supports two formatting styles:
 * - SqlStyle: sqlstyle.guide conventions with river alignment
 * - PgFormatter: pgFormatter-compatible style with many options
 * - Compact: Minimal whitespace, no additional options
 */
export function FormatterSettings({ value, onChange, compact = false }: FormatterSettingsProps) {
  const [showAdvanced, setShowAdvanced] = useState(false);

  const handleStyleChange = (style: string | null) => {
    if (!style) return;
    const newConfig = getDefaultConfigForStyle(style as FormatterStyle);
    onChange(newConfig);
  };

  const updateSqlStyle = (updates: Partial<SqlStyleConfig>) => {
    if (value.style !== 'sql_style') return;
    onChange({ ...value, ...updates } as SqlStyleConfig);
  };

  const updatePgFormatter = (updates: Partial<PgFormatterConfig>) => {
    if (value.style !== 'pg_formatter') return;
    onChange({ ...value, ...updates } as PgFormatterConfig);
  };

  const styleOptions = Object.entries(FORMATTER_STYLE_LABELS).map(([v, label]) => ({
    value: v,
    label,
  }));

  return (
    <Stack gap={compact ? 'xs' : 'md'}>
      <Select
        label="Formatter Style"
        description={compact ? undefined : 'Choose the formatting style to use'}
        value={value.style}
        onChange={handleStyleChange}
        data={styleOptions}
        size={compact ? 'xs' : 'sm'}
      />

      {value.style === 'sql_style' && (
        <SqlStyleSettings
          config={value}
          onChange={updateSqlStyle}
          compact={compact}
          showAdvanced={showAdvanced}
          onToggleAdvanced={() => setShowAdvanced(!showAdvanced)}
        />
      )}

      {value.style === 'pg_formatter' && (
        <PgFormatterSettings
          config={value}
          onChange={updatePgFormatter}
          compact={compact}
          showAdvanced={showAdvanced}
          onToggleAdvanced={() => setShowAdvanced(!showAdvanced)}
        />
      )}

      {value.style === 'compact' && !compact && (
        <Text size="sm" c="dimmed">
          Compact style uses minimal whitespace with no additional options.
        </Text>
      )}
    </Stack>
  );
}

// =============================================================================
// SqlStyle Settings
// =============================================================================

interface SqlStyleSettingsProps {
  config: SqlStyleConfig;
  onChange: (updates: Partial<SqlStyleConfig>) => void;
  compact: boolean;
  showAdvanced: boolean;
  onToggleAdvanced: () => void;
}

function SqlStyleSettings({
  config,
  onChange,
  compact,
  showAdvanced,
  onToggleAdvanced,
}: SqlStyleSettingsProps) {
  const keywordCaseOptions = Object.entries(KEYWORD_CASE_LABELS).map(([v, label]) => ({
    value: v,
    label,
  }));

  const identifierCaseOptions = Object.entries(IDENTIFIER_CASE_LABELS).map(([v, label]) => ({
    value: v,
    label,
  }));

  const commaStyleOptions = Object.entries(COMMA_STYLE_LABELS).map(([v, label]) => ({
    value: v,
    label,
  }));

  return (
    <>
      <Select
        label="Keyword Case"
        value={config.keywordCase}
        onChange={(v) => onChange({ keywordCase: v as KeywordCase })}
        data={keywordCaseOptions}
        size={compact ? 'xs' : 'sm'}
      />

      <Group grow>
        <NumberInput
          label="Indent Spaces"
          value={config.indentSpaces}
          onChange={(v) => onChange({ indentSpaces: Number(v) || 4 })}
          min={1}
          max={8}
          size={compact ? 'xs' : 'sm'}
        />
        <Switch
          label="Use Tabs"
          checked={config.useTabs}
          onChange={(e) => onChange({ useTabs: e.currentTarget.checked })}
          size={compact ? 'xs' : 'sm'}
          mt={compact ? 20 : 24}
        />
      </Group>

      <Select
        label="Comma Style"
        value={config.commaStyle}
        onChange={(v) => onChange({ commaStyle: v as CommaStyle })}
        data={commaStyleOptions}
        size={compact ? 'xs' : 'sm'}
      />

      <Switch
        label="River Alignment"
        description={compact ? undefined : 'Align keywords in a river pattern'}
        checked={config.riverAlignment}
        onChange={(e) => onChange({ riverAlignment: e.currentTarget.checked })}
        size={compact ? 'xs' : 'sm'}
      />

      {!compact && (
        <>
          <Divider />
          <Group gap="xs" style={{ cursor: 'pointer' }} onClick={onToggleAdvanced}>
            <ActionIcon variant="subtle" size="xs">
              {showAdvanced ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
            </ActionIcon>
            <Text size="sm" fw={500}>
              Advanced Options
            </Text>
          </Group>

          <Collapse in={showAdvanced}>
            <Stack gap="md">
              <Select
                label="Identifier Case"
                value={config.identifierCase}
                onChange={(v) => onChange({ identifierCase: v as IdentifierCase })}
                data={identifierCaseOptions}
                size="sm"
              />

              <NumberInput
                label="Max Line Width"
                value={config.maxWidth}
                onChange={(v) => onChange({ maxWidth: Number(v) || 88 })}
                min={40}
                max={200}
                size="sm"
              />

              <Switch
                label="Newline Before AND/OR"
                checked={config.newlineBeforeLogical}
                onChange={(e) => onChange({ newlineBeforeLogical: e.currentTarget.checked })}
                size="sm"
              />

              <Switch
                label="Spaces Around Operators"
                checked={config.spacesAroundOperators}
                onChange={(e) => onChange({ spacesAroundOperators: e.currentTarget.checked })}
                size="sm"
              />

              <Switch
                label="Parentheses Spacing"
                checked={config.parenthesesSpacing}
                onChange={(e) => onChange({ parenthesesSpacing: e.currentTarget.checked })}
                size="sm"
              />

              <Switch
                label="Align SELECT Items"
                checked={config.alignSelectItems}
                onChange={(e) => onChange({ alignSelectItems: e.currentTarget.checked })}
                size="sm"
              />

              <NumberInput
                label="River Width"
                value={config.riverWidth}
                onChange={(v) => onChange({ riverWidth: Number(v) || 10 })}
                min={5}
                max={20}
                size="sm"
              />
            </Stack>
          </Collapse>
        </>
      )}
    </>
  );
}

// =============================================================================
// PgFormatter Settings
// =============================================================================

interface PgFormatterSettingsProps {
  config: PgFormatterConfig;
  onChange: (updates: Partial<PgFormatterConfig>) => void;
  compact: boolean;
  showAdvanced: boolean;
  onToggleAdvanced: () => void;
}

function PgFormatterSettings({
  config,
  onChange,
  compact,
  showAdvanced,
  onToggleAdvanced,
}: PgFormatterSettingsProps) {
  const pgCaseOptions = Object.entries(PG_CASE_OPTION_LABELS).map(([v, label]) => ({
    value: v,
    label,
  }));

  return (
    <>
      <Select
        label="Keyword Case"
        value={String(config.keywordCase)}
        onChange={(v) => onChange({ keywordCase: Number(v) as PgCaseOption })}
        data={pgCaseOptions}
        size={compact ? 'xs' : 'sm'}
      />

      <Group grow>
        <NumberInput
          label="Indent Spaces"
          value={config.spaces}
          onChange={(v) => onChange({ spaces: Number(v) || 4 })}
          min={1}
          max={8}
          size={compact ? 'xs' : 'sm'}
        />
        <Switch
          label="Use Tabs"
          checked={config.useTabs}
          onChange={(e) => onChange({ useTabs: e.currentTarget.checked })}
          size={compact ? 'xs' : 'sm'}
          mt={compact ? 20 : 24}
        />
      </Group>

      <Switch
        label="Comma at Start of Line"
        checked={config.commaStart}
        onChange={(e) =>
          onChange({
            commaStart: e.currentTarget.checked,
            commaEnd: !e.currentTarget.checked,
          })
        }
        size={compact ? 'xs' : 'sm'}
      />

      {!compact && (
        <>
          <Divider />
          <Group gap="xs" style={{ cursor: 'pointer' }} onClick={onToggleAdvanced}>
            <ActionIcon variant="subtle" size="xs">
              {showAdvanced ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
            </ActionIcon>
            <Text size="sm" fw={500}>
              Advanced Options
            </Text>
          </Group>

          <Collapse in={showAdvanced}>
            <Stack gap="md">
              <Text size="sm" fw={500} c="dimmed">
                Case Options
              </Text>

              <Select
                label="Function Case"
                value={String(config.functionCase)}
                onChange={(v) => onChange({ functionCase: Number(v) as PgCaseOption })}
                data={pgCaseOptions}
                size="sm"
              />

              <Select
                label="Type Case"
                value={String(config.typeCase)}
                onChange={(v) => onChange({ typeCase: Number(v) as PgCaseOption })}
                data={pgCaseOptions}
                size="sm"
              />

              <Divider />
              <Text size="sm" fw={500} c="dimmed">
                Line Wrapping
              </Text>

              <NumberInput
                label="Wrap Limit (characters)"
                value={config.wrapLimit ?? ''}
                onChange={(v) => onChange({ wrapLimit: v ? Number(v) : null })}
                min={40}
                max={200}
                placeholder="No limit"
                size="sm"
              />

              <NumberInput
                label="Wrap After (items)"
                value={config.wrapAfter ?? ''}
                onChange={(v) => onChange({ wrapAfter: v ? Number(v) : null })}
                min={1}
                max={20}
                placeholder="No limit"
                size="sm"
              />

              <Switch
                label="Wrap Comments"
                checked={config.wrapComment}
                onChange={(e) => onChange({ wrapComment: e.currentTarget.checked })}
                size="sm"
              />

              <Divider />
              <Text size="sm" fw={500} c="dimmed">
                Content Handling
              </Text>

              <Switch
                label="Remove Comments"
                checked={config.noComment}
                onChange={(e) => onChange({ noComment: e.currentTarget.checked })}
                size="sm"
              />

              <Switch
                label="Keep Original Empty Lines"
                checked={config.keepNewline}
                onChange={(e) => onChange({ keepNewline: e.currentTarget.checked })}
                size="sm"
              />

              <Switch
                label="No Trailing Newline"
                checked={config.noExtraLine}
                onChange={(e) => onChange({ noExtraLine: e.currentTarget.checked })}
                size="sm"
              />

              <Switch
                label="Newline Between Statements"
                checked={config.noGrouping}
                onChange={(e) => onChange({ noGrouping: e.currentTarget.checked })}
                size="sm"
              />

              <Divider />
              <Text size="sm" fw={500} c="dimmed">
                Special Options
              </Text>

              <Switch
                label="No Space Before Function Parens"
                checked={config.noSpaceFunction}
                onChange={(e) => onChange({ noSpaceFunction: e.currentTarget.checked })}
                size="sm"
              />

              <Switch
                label="Keep Redundant Parentheses"
                checked={config.redundantParenthesis}
                onChange={(e) => onChange({ redundantParenthesis: e.currentTarget.checked })}
                size="sm"
              />

              <Switch
                label="River Alignment"
                checked={config.riverAlignment}
                onChange={(e) => onChange({ riverAlignment: e.currentTarget.checked })}
                size="sm"
              />

              <NumberInput
                label="Max Line Width"
                value={config.maxWidth}
                onChange={(v) => onChange({ maxWidth: Number(v) || 80 })}
                min={40}
                max={200}
                size="sm"
              />
            </Stack>
          </Collapse>
        </>
      )}
    </>
  );
}

export default FormatterSettings;

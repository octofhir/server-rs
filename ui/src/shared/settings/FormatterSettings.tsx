import {
  Select,
  Switch,
  NumberInput,
  Text,
  Divider,
  Collapse,
  ActionIcon,
} from "@octofhir/ui-kit";
import { ChevronDown, ChevronRight } from '@gravity-ui/icons';
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
import classes from './FormatterSettings.module.css';

function isFormatterStyle(value: string | null): value is FormatterStyle {
  return value !== null && value in FORMATTER_STYLE_LABELS;
}

function isKeywordCase(value: string | null): value is KeywordCase {
  return value !== null && value in KEYWORD_CASE_LABELS;
}

function isIdentifierCase(value: string | null): value is IdentifierCase {
  return value !== null && value in IDENTIFIER_CASE_LABELS;
}

function isCommaStyle(value: string | null): value is CommaStyle {
  return value !== null && value in COMMA_STYLE_LABELS;
}

function parsePgCaseOption(value: string | null): PgCaseOption | null {
  if (value === null) return null;
  const numeric = Number(value);
  return numeric === 0 || numeric === 1 || numeric === 2 || numeric === 3 ? numeric : null;
}

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
    if (!isFormatterStyle(style)) return;
    const newConfig = getDefaultConfigForStyle(style);
    onChange(newConfig);
  };

  const updateSqlStyle = (updates: Partial<SqlStyleConfig>) => {
    if (value.style !== 'sql_style') return;
    onChange({ ...value, ...updates });
  };

  const updatePgFormatter = (updates: Partial<PgFormatterConfig>) => {
    if (value.style !== 'pg_formatter') return;
    onChange({ ...value, ...updates });
  };

  const styleOptions = Object.entries(FORMATTER_STYLE_LABELS).map(([v, label]) => ({
    value: v,
    label,
  }));

  return (
    <div className={compact ? classes.compactRoot : classes.root}>
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
    </div>
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
        onChange={(value) => {
          if (isKeywordCase(value)) {
            onChange({ keywordCase: value });
          }
        }}
        data={keywordCaseOptions}
        size={compact ? 'xs' : 'sm'}
      />

      <div className={classes.pairedControls}>
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
          className={classes.inlineSwitch}
        />
      </div>

      <Select
        label="Comma Style"
        value={config.commaStyle}
        onChange={(value) => {
          if (isCommaStyle(value)) {
            onChange({ commaStyle: value });
          }
        }}
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
          <button type="button" className={classes.advancedToggle} onClick={onToggleAdvanced}>
            <ActionIcon variant="subtle" size="xs">
              {showAdvanced ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            </ActionIcon>
            <Text size="sm" fw={500}>
              Advanced Options
            </Text>
          </button>

          <Collapse in={showAdvanced}>
            <div className={classes.advancedPanel}>
              <Select
                label="Identifier Case"
                value={config.identifierCase}
                onChange={(value) => {
                  if (isIdentifierCase(value)) {
                    onChange({ identifierCase: value });
                  }
                }}
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
            </div>
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
        onChange={(value) => {
          const next = parsePgCaseOption(value);
          if (next !== null) {
            onChange({ keywordCase: next });
          }
        }}
        data={pgCaseOptions}
        size={compact ? 'xs' : 'sm'}
      />

      <div className={classes.pairedControls}>
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
          className={classes.inlineSwitch}
        />
      </div>

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
          <button type="button" className={classes.advancedToggle} onClick={onToggleAdvanced}>
            <ActionIcon variant="subtle" size="xs">
              {showAdvanced ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            </ActionIcon>
            <Text size="sm" fw={500}>
              Advanced Options
            </Text>
          </button>

          <Collapse in={showAdvanced}>
            <div className={classes.advancedPanel}>
              <Text size="sm" fw={500} c="dimmed">
                Case Options
              </Text>

              <Select
                label="Function Case"
                value={String(config.functionCase)}
                onChange={(value) => {
                  const next = parsePgCaseOption(value);
                  if (next !== null) {
                    onChange({ functionCase: next });
                  }
                }}
                data={pgCaseOptions}
                size="sm"
              />

              <Select
                label="Type Case"
                value={String(config.typeCase)}
                onChange={(value) => {
                  const next = parsePgCaseOption(value);
                  if (next !== null) {
                    onChange({ typeCase: next });
                  }
                }}
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
            </div>
          </Collapse>
        </>
      )}
    </>
  );
}

export default FormatterSettings;

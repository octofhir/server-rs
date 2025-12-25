/**
 * SQL Formatter Configuration Types
 *
 * These types match the Rust structs in crates/octofhir-server/src/lsp/formatter_config.rs
 */

// =============================================================================
// SqlStyle Configuration
// =============================================================================

export type KeywordCase = 'upper' | 'lower' | 'preserve';
export type IdentifierCase = 'lower' | 'preserve';
export type CommaStyle = 'trailing' | 'leading';

/**
 * SqlStyle formatter configuration.
 * Follows sqlstyle.guide conventions with river alignment.
 */
export interface SqlStyleConfig {
  style: 'sql_style';
  /** Keyword case transformation (default: "upper") */
  keywordCase: KeywordCase;
  /** Identifier case transformation (default: "lower") */
  identifierCase: IdentifierCase;
  /** Number of spaces per indent level (default: 4) */
  indentSpaces: number;
  /** Use tabs instead of spaces (default: false) */
  useTabs: boolean;
  /** Maximum line width (default: 88) */
  maxWidth: number;
  /** Use river alignment for keywords (default: true) */
  riverAlignment: boolean;
  /** Insert newline before AND/OR (default: true) */
  newlineBeforeLogical: boolean;
  /** Add spaces around operators (default: true) */
  spacesAroundOperators: boolean;
  /** Comma placement style (default: "trailing") */
  commaStyle: CommaStyle;
  /** Add space inside parentheses (default: false) */
  parenthesesSpacing: boolean;
  /** Align items in SELECT list (default: true) */
  alignSelectItems: boolean;
  /** River width for alignment (default: 10) */
  riverWidth: number;
}

// =============================================================================
// PgFormatter Configuration
// =============================================================================

/**
 * Case option for pgFormatter (matches pgFormatter's 0-3 values).
 * 0 = unchanged, 1 = lower, 2 = upper, 3 = capitalize
 */
export type PgCaseOption = 0 | 1 | 2 | 3;

/**
 * PgFormatter configuration.
 * Compatible with the pgFormatter tool.
 */
export interface PgFormatterConfig {
  style: 'pg_formatter';

  // === Case Options ===
  /** Keyword case: 0=unchanged, 1=lower, 2=upper, 3=capitalize (default: 2) */
  keywordCase: PgCaseOption;
  /** Function name case: 0=unchanged, 1=lower, 2=upper, 3=capitalize (default: 0) */
  functionCase: PgCaseOption;
  /** Data type case: 0=unchanged, 1=lower, 2=upper, 3=capitalize (default: 1) */
  typeCase: PgCaseOption;

  // === Indentation ===
  /** Number of spaces per indent level (default: 4) */
  spaces: number;
  /** Use tabs instead of spaces (default: false) */
  useTabs: boolean;

  // === Comma Placement ===
  /** Place comma at beginning of line (default: false) */
  commaStart: boolean;
  /** Place comma at end of line (default: true) */
  commaEnd: boolean;
  /** Add newline after each comma (default: false) */
  commaBreak: boolean;

  // === Line Wrapping ===
  /** Wrap lines at N characters (default: null) */
  wrapLimit: number | null;
  /** Wrap lists after N items (default: null) */
  wrapAfter: number | null;
  /** Apply wrap limit to comments (default: false) */
  wrapComment: boolean;

  // === Content Handling ===
  /** Remove all comments (default: false) */
  noComment: boolean;
  /** Keep original empty lines (default: false) */
  keepNewline: boolean;
  /** Do not add trailing newline at end (default: false) */
  noExtraLine: boolean;
  /** Add newline between statements (default: false) */
  noGrouping: boolean;

  // === Special Options ===
  /** Do not add space before function parentheses (default: false) */
  noSpaceFunction: boolean;
  /** Keep redundant parentheses (default: false) */
  redundantParenthesis: boolean;
  /** Regex pattern to protect from formatting (default: null) */
  placeholder: string | null;

  // === Extra Options ===
  /** Maximum line width for formatting (default: 80) */
  maxWidth: number;
  /** Use river alignment for keywords (default: false) */
  riverAlignment: boolean;
}

// =============================================================================
// Compact Configuration
// =============================================================================

/**
 * Compact formatter configuration.
 * Uses minimal whitespace with no additional options.
 */
export interface CompactConfig {
  style: 'compact';
}

// =============================================================================
// Union Type
// =============================================================================

/**
 * Union type for all formatter configurations.
 */
export type FormatterConfig = SqlStyleConfig | PgFormatterConfig | CompactConfig;

/**
 * Style type for the formatter.
 */
export type FormatterStyle = 'sql_style' | 'pg_formatter' | 'compact';

// =============================================================================
// Default Configurations
// =============================================================================

/**
 * Default SqlStyle configuration.
 */
export const DEFAULT_SQL_STYLE_CONFIG: SqlStyleConfig = {
  style: 'sql_style',
  keywordCase: 'upper',
  identifierCase: 'lower',
  indentSpaces: 4,
  useTabs: false,
  maxWidth: 88,
  riverAlignment: true,
  newlineBeforeLogical: true,
  spacesAroundOperators: true,
  commaStyle: 'trailing',
  parenthesesSpacing: false,
  alignSelectItems: true,
  riverWidth: 10,
};

/**
 * Default PgFormatter configuration.
 */
export const DEFAULT_PG_FORMATTER_CONFIG: PgFormatterConfig = {
  style: 'pg_formatter',
  keywordCase: 2,
  functionCase: 0,
  typeCase: 1,
  spaces: 4,
  useTabs: false,
  commaStart: false,
  commaEnd: true,
  commaBreak: false,
  wrapLimit: null,
  wrapAfter: null,
  wrapComment: false,
  noComment: false,
  keepNewline: false,
  noExtraLine: false,
  noGrouping: false,
  noSpaceFunction: false,
  redundantParenthesis: false,
  placeholder: null,
  maxWidth: 80,
  riverAlignment: false,
};

/**
 * Default Compact configuration.
 */
export const DEFAULT_COMPACT_CONFIG: CompactConfig = {
  style: 'compact',
};

/**
 * Default formatter configuration.
 */
export const DEFAULT_FORMATTER_CONFIG: FormatterConfig = DEFAULT_SQL_STYLE_CONFIG;

/**
 * Get default config for a given style.
 */
export function getDefaultConfigForStyle(style: FormatterStyle): FormatterConfig {
  switch (style) {
    case 'sql_style':
      return { ...DEFAULT_SQL_STYLE_CONFIG };
    case 'pg_formatter':
      return { ...DEFAULT_PG_FORMATTER_CONFIG };
    case 'compact':
      return { ...DEFAULT_COMPACT_CONFIG };
  }
}

// =============================================================================
// Helper functions
// =============================================================================

/**
 * Labels for pgFormatter case options.
 */
export const PG_CASE_OPTION_LABELS: Record<PgCaseOption, string> = {
  0: 'Unchanged',
  1: 'lowercase',
  2: 'UPPERCASE',
  3: 'Capitalize',
};

/**
 * Labels for keyword case options.
 */
export const KEYWORD_CASE_LABELS: Record<KeywordCase, string> = {
  upper: 'UPPERCASE',
  lower: 'lowercase',
  preserve: 'Preserve',
};

/**
 * Labels for identifier case options.
 */
export const IDENTIFIER_CASE_LABELS: Record<IdentifierCase, string> = {
  lower: 'lowercase',
  preserve: 'Preserve',
};

/**
 * Labels for comma style options.
 */
export const COMMA_STYLE_LABELS: Record<CommaStyle, string> = {
  trailing: 'Trailing (end of line)',
  leading: 'Leading (start of line)',
};

/**
 * Labels for formatter styles.
 */
export const FORMATTER_STYLE_LABELS: Record<FormatterStyle, string> = {
  sql_style: 'SQL Style (sqlstyle.guide)',
  pg_formatter: 'pgFormatter',
  compact: 'Compact',
};

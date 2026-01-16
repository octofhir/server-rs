import { useRef, useEffect, useCallback } from "react";
import Editor, { type OnMount, type OnChange } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import { useMantineColorScheme } from "@mantine/core";

export interface AutomationScriptEditorProps {
  /** Script content */
  value?: string;
  /** Callback when content changes */
  onChange?: (value: string) => void;
  /** Callback when Ctrl+Enter is pressed */
  onExecute?: () => void;
  /** Callback when Ctrl+S is pressed */
  onSave?: () => void;
  /** Editor height (default: 400px) */
  height?: string | number;
  /** Whether the editor is read-only */
  readOnly?: boolean;
  /** Custom CSS class for the container */
  className?: string;
}

/**
 * Type definitions for automation runtime APIs.
 * These provide autocomplete for built-in functions and context variables.
 */
const AUTOMATION_SCRIPT_TYPES = `
// =============================================================================
// Automation Context (passed to export default async function)
// =============================================================================

/**
 * Context object passed to the automation function.
 * Contains the event that triggered the automation.
 */
interface AutomationContext {
  /** The event that triggered this automation */
  event: AutomationEvent;
}

/**
 * The event that triggered this automation.
 * Contains information about what happened and the affected resource.
 */
interface AutomationEvent {
  /**
   * Type of event that triggered this automation.
   * - "created": A new resource was created
   * - "updated": An existing resource was modified
   * - "deleted": A resource was deleted
   * - "manual": Automation was triggered manually (via API or playground)
   */
  type: "created" | "updated" | "deleted" | "manual";

  /**
   * The FHIR resource that triggered the event.
   * For "created" events, this is the newly created resource.
   * For "updated" events, this is the updated resource (new version).
   * For "deleted" events, this is the resource that was deleted.
   */
  resource: FhirResource;

  /**
   * Previous version of the resource (only for "updated" events).
   * Useful for detecting what changed between versions.
   */
  previous?: FhirResource;

  /**
   * ISO 8601 timestamp when the event occurred.
   */
  timestamp: string;
}

// =============================================================================
// FHIR Client API
// =============================================================================

/**
 * FHIR client for performing CRUD operations on resources.
 * All operations are synchronous within the automation context.
 */
declare const fhir: {
  /**
   * Create a new FHIR resource.
   *
   * @param resource - The resource to create (must include resourceType)
   * @returns The created resource with server-assigned id
   *
   * @example
   * const task = fhir.create({
   *   resourceType: "Task",
   *   status: "requested",
   *   intent: "order",
   *   description: "Welcome task for new patient"
   * });
   * console.log("Created task:", task.id);
   */
  create<T extends FhirResource>(resource: T): T;

  /**
   * Read a FHIR resource by type and ID.
   *
   * @param resourceType - The type of resource (e.g., "Patient", "Observation")
   * @param id - The resource ID
   * @returns The resource if found
   * @throws Error if resource not found
   *
   * @example
   * const patient = fhir.read("Patient", "123");
   * console.log("Patient name:", patient.name?.[0]?.text);
   */
  read<T extends FhirResource>(resourceType: string, id: string): T;

  /**
   * Update an existing FHIR resource.
   *
   * @param resource - The resource to update (must include resourceType and id)
   * @returns The updated resource
   *
   * @example
   * const patient = fhir.read("Patient", "123");
   * patient.active = false;
   * fhir.update(patient);
   */
  update<T extends FhirResource>(resource: T): T;

  /**
   * Delete a FHIR resource.
   *
   * @param resourceType - The type of resource to delete
   * @param id - The resource ID
   *
   * @example
   * fhir.delete("Task", "old-task-id");
   */
  delete(resourceType: string, id: string): void;

  /**
   * Search for FHIR resources.
   *
   * @param resourceType - The type of resource to search for
   * @param params - Search parameters as key-value pairs
   * @returns A Bundle containing matching resources
   *
   * @example
   * const result = fhir.search("Observation", {
   *   subject: "Patient/123",
   *   code: "http://loinc.org|12345-6"
   * });
   * for (const entry of result.entry || []) {
   *   console.log("Found:", entry.resource.id);
   * }
   */
  search(resourceType: string, params?: Record<string, string>): FhirBundle;

  /**
   * Patch a FHIR resource with partial updates.
   *
   * @param resourceType - The type of resource to patch
   * @param id - The resource ID
   * @param patch - Partial resource with fields to update
   * @returns The patched resource
   *
   * @example
   * fhir.patch("Patient", "123", { active: false });
   */
  patch<T extends FhirResource>(resourceType: string, id: string, patch: Partial<T>): T;
};

// =============================================================================
// Native Fetch API
// =============================================================================

/**
 * Native fetch API for making HTTP requests.
 * Standard web fetch - use await for async operations.
 *
 * @example
 * const response = await fetch("https://api.example.com/webhook", {
 *   method: "POST",
 *   headers: { "Content-Type": "application/json" },
 *   body: JSON.stringify({ patientId: ctx.event.resource.id })
 * });
 * if (!response.ok) {
 *   console.error("Webhook failed:", response.status);
 * }
 * const data = await response.json();
 */
declare function fetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response>;

// =============================================================================
// Console (Logging)
// =============================================================================

/**
 * Console for logging messages during automation execution.
 * Note: console.log output may not be captured. Use execution.log() for reliable logging.
 */
declare const console: {
  /** Log a message at info level */
  log(...args: any[]): void;
  /** Log an info message */
  info(...args: any[]): void;
  /** Log a debug message */
  debug(...args: any[]): void;
  /** Log a warning message */
  warn(...args: any[]): void;
  /** Log an error message */
  error(...args: any[]): void;
};

// =============================================================================
// Execution Logging API
// =============================================================================

/**
 * Execution logging API for structured logs.
 * Logs are captured and stored in the execution record.
 * Use this instead of console.log for reliable logging that's visible in the execution history.
 *
 * @example
 * execution.log("Processing patient", { patientId: ctx.event.resource.id });
 * execution.info("Validation passed");
 * execution.warn("Missing optional field", { field: "birthDate" });
 * execution.error("Failed to create task", { error: e.message });
 */
declare const execution: {
  /**
   * Log a message at default level.
   * @param message - The log message
   * @param data - Optional structured data to attach (will be stored as JSON)
   *
   * @example
   * execution.log("Processing resource", { id: "123", type: "Patient" });
   */
  log(message: string, data?: unknown): void;

  /**
   * Log an informational message.
   * @param message - The log message
   * @param data - Optional structured data to attach
   *
   * @example
   * execution.info("Task created successfully", { taskId: task.id });
   */
  info(message: string, data?: unknown): void;

  /**
   * Log a debug message.
   * @param message - The log message
   * @param data - Optional structured data to attach
   *
   * @example
   * execution.debug("Raw event data", ctx.event);
   */
  debug(message: string, data?: unknown): void;

  /**
   * Log a warning message.
   * @param message - The log message
   * @param data - Optional structured data to attach
   *
   * @example
   * execution.warn("Missing expected field", { field: "birthDate", resource: "Patient" });
   */
  warn(message: string, data?: unknown): void;

  /**
   * Log an error message.
   * @param message - The log message
   * @param data - Optional structured data to attach
   *
   * @example
   * execution.error("Failed to process", { error: e.message, stack: e.stack });
   */
  error(message: string, data?: unknown): void;
};

// =============================================================================
// FHIR Resource Types
// =============================================================================

interface FhirResource {
  /** Resource type (e.g., "Patient", "Observation") */
  resourceType: string;
  /** Resource ID */
  id?: string;
  /** Resource metadata */
  meta?: {
    versionId?: string;
    lastUpdated?: string;
    profile?: string[];
  };
  /** Allow any additional properties */
  [key: string]: any;
}

interface FhirBundle {
  resourceType: "Bundle";
  id?: string;
  type: string;
  total?: number;
  link?: Array<{
    relation: string;
    url: string;
  }>;
  entry?: Array<{
    resource: FhirResource;
    fullUrl?: string;
    search?: { mode: string };
  }>;
}

// Common FHIR resource type hints
interface Patient extends FhirResource {
  resourceType: "Patient";
  identifier?: Array<{ system?: string; value?: string }>;
  active?: boolean;
  name?: Array<{
    use?: string;
    text?: string;
    family?: string;
    given?: string[];
  }>;
  telecom?: Array<{ system?: string; value?: string; use?: string }>;
  gender?: "male" | "female" | "other" | "unknown";
  birthDate?: string;
  address?: Array<{
    use?: string;
    text?: string;
    line?: string[];
    city?: string;
    state?: string;
    postalCode?: string;
    country?: string;
  }>;
}

interface Observation extends FhirResource {
  resourceType: "Observation";
  status: "registered" | "preliminary" | "final" | "amended" | "corrected" | "cancelled" | "entered-in-error" | "unknown";
  code: CodeableConcept;
  subject?: Reference;
  effectiveDateTime?: string;
  valueQuantity?: { value?: number; unit?: string; system?: string; code?: string };
  valueString?: string;
  valueBoolean?: boolean;
  valueCodeableConcept?: CodeableConcept;
}

interface Task extends FhirResource {
  resourceType: "Task";
  status: "draft" | "requested" | "received" | "accepted" | "rejected" | "ready" | "cancelled" | "in-progress" | "on-hold" | "failed" | "completed" | "entered-in-error";
  intent: "unknown" | "proposal" | "plan" | "order" | "original-order" | "reflex-order" | "filler-order" | "instance-order" | "option";
  description?: string;
  for?: Reference;
  authoredOn?: string;
  requester?: Reference;
  owner?: Reference;
}

interface Encounter extends FhirResource {
  resourceType: "Encounter";
  status: "planned" | "arrived" | "triaged" | "in-progress" | "onleave" | "finished" | "cancelled" | "entered-in-error" | "unknown";
  class: Coding;
  subject?: Reference;
  period?: { start?: string; end?: string };
}

interface CodeableConcept {
  coding?: Coding[];
  text?: string;
}

interface Coding {
  system?: string;
  code?: string;
  display?: string;
}

interface Reference {
  reference?: string;
  display?: string;
}
`;

/**
 * Default code template for new automations.
 * Shows the required export default async function format.
 */
export const DEFAULT_AUTOMATION_CODE = `/**
 * Automation Script
 *
 * Available global APIs:
 * - fhir: FHIR client (create, read, update, delete, search, patch)
 * - fetch: Native fetch API for HTTP requests
 * - execution: Structured logging (log, info, debug, warn, error)
 *
 * @param ctx - Context with the triggering event
 * @param ctx.event - Event: { type, resource, previous?, timestamp }
 */
export default async function(ctx: AutomationContext) {
  const { event } = ctx;

  execution.log("Processing event", { type: event.type, resourceType: event.resource.resourceType });

  if (event.type === "created" && event.resource.resourceType === "Patient") {
    const patient = event.resource;

    // Create a welcome task for the new patient
    const task = fhir.create({
      resourceType: "Task",
      status: "requested",
      intent: "order",
      description: \`Welcome patient \${patient.name?.[0]?.text || "Unknown"}\`,
      for: { reference: \`Patient/\${patient.id}\` }
    });

    execution.info("Created welcome task", { taskId: task.id, patientId: patient.id });
  }
}
`;

/**
 * Validation result for automation code.
 */
export interface ValidationResult {
  valid: boolean;
  error?: string;
}

/**
 * Validates automation code format.
 * Checks for required export default async function.
 */
export function validateAutomationCode(code: string): ValidationResult {
  // Check for export default
  if (!code.includes("export default")) {
    return {
      valid: false,
      error: 'Automation must have "export default async function"',
    };
  }

  // Check that it's an async function
  const exportMatch = code.match(/export\s+default\s+(async\s+)?function/);
  if (!exportMatch || !exportMatch[1]) {
    return {
      valid: false,
      error: "Default export must be an async function",
    };
  }

  return { valid: true };
}

// Flag to track if we've registered types
let typesRegistered = false;

/**
 * React wrapper for Monaco JavaScript Editor for Automation Scripts.
 * Provides syntax highlighting, autocomplete for built-in functions,
 * and context variable documentation.
 */
export function AutomationScriptEditor({
  value = "",
  onChange,
  onExecute,
  onSave,
  height = 400,
  readOnly = false,
  className,
}: AutomationScriptEditorProps) {
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<typeof Monaco | null>(null);
  const { colorScheme } = useMantineColorScheme();
  const editorTheme = colorScheme === "dark" ? "vs-dark" : "vs";

  // Setup Monaco when editor mounts
  const handleEditorDidMount: OnMount = useCallback(
    (editor, monaco) => {
      editorRef.current = editor;
      monacoRef.current = monaco;

      // Register type definitions for autocomplete (only once)
      if (!typesRegistered) {
        monaco.languages.typescript.typescriptDefaults.addExtraLib(
          AUTOMATION_SCRIPT_TYPES,
          "automation-globals.d.ts",
        );

        // Configure TypeScript/JavaScript compiler options
        monaco.languages.typescript.typescriptDefaults.setCompilerOptions({
          target: monaco.languages.typescript.ScriptTarget.ES2020,
          allowNonTsExtensions: true,
          moduleResolution: monaco.languages.typescript.ModuleResolutionKind.NodeJs,
          noEmit: true,
          strict: false,
        });

        // Enable diagnostics
        monaco.languages.typescript.typescriptDefaults.setDiagnosticsOptions({
          noSemanticValidation: false,
          noSyntaxValidation: false,
        });

        typesRegistered = true;
      }

      // Add Ctrl+Enter to execute
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => {
        onExecute?.();
      });

      // Add Ctrl+S to save
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        onSave?.();
      });

      // Focus the editor
      editor.focus();
    },
    [onExecute, onSave],
  );

  // Handle value changes from React
  const handleChange: OnChange = useCallback(
    (newValue) => {
      onChange?.(newValue ?? "");
    },
    [onChange],
  );

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      editorRef.current?.dispose();
    };
  }, []);

  return (
    <div className={className} style={{ height, width: "100%" }}>
      <Editor
        height="100%"
        language="typescript"
        theme={editorTheme}
        value={value}
        onChange={handleChange}
        onMount={handleEditorDidMount}
        options={{
          automaticLayout: true,
          minimap: { enabled: false },
          lineNumbers: "on",
          renderLineHighlight: "line",
          scrollBeyondLastLine: false,
          fontSize: 13,
          fontFamily: "var(--font-mono, 'JetBrains Mono', 'Fira Code', monospace)",
          tabSize: 2,
          insertSpaces: true,
          wordWrap: "on",
          readOnly,
          padding: { top: 8, bottom: 8 },
          suggestOnTriggerCharacters: true,
          quickSuggestions: {
            other: true,
            comments: true,
            strings: true,
          },
          acceptSuggestionOnEnter: "on",
          parameterHints: { enabled: true },
          suggest: {
            showKeywords: true,
            showSnippets: true,
            showFunctions: true,
            showVariables: true,
            showConstants: true,
            showMethods: true,
            showProperties: true,
          },
        }}
      />
    </div>
  );
}

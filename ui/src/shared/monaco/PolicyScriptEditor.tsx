import { useRef, useEffect, useCallback } from "react";
import Editor, { type OnMount, type OnChange } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import { useMantineColorScheme } from "@mantine/core";

export interface PolicyScriptEditorProps {
	/** Script content */
	value?: string;
	/** Callback when content changes */
	onChange?: (value: string) => void;
	/** Editor height (default: 200px) */
	height?: string | number;
	/** Whether the editor is read-only */
	readOnly?: boolean;
	/** Custom CSS class for the container */
	className?: string;
	/** Placeholder text when empty */
	placeholder?: string;
}

/**
 * Type definitions for QuickJS policy scripts.
 * These provide autocomplete for built-in functions and context variables.
 */
const POLICY_SCRIPT_TYPES = `
// =============================================================================
// Decision Helper Functions
// =============================================================================

/**
 * Return an allow decision.
 * Use this when the request should be permitted.
 *
 * @example
 * if (hasRole("admin")) {
 *   return allow();
 * }
 */
declare function allow(): { decision: "allow" };

/**
 * Return a deny decision with an optional reason.
 * Use this when the request should be blocked.
 *
 * @param reason - Optional message explaining why access was denied
 * @example
 * if (!hasRole("admin")) {
 *   return deny("Admin role required");
 * }
 */
declare function deny(reason?: string): { decision: "deny"; reason: string };

/**
 * Return an abstain decision.
 * Use this when this policy doesn't apply and evaluation should continue to next policy.
 *
 * @example
 * if (request.resourceType !== "Patient") {
 *   return abstain(); // Let other policies handle non-Patient resources
 * }
 */
declare function abstain(): { decision: "abstain" };

// =============================================================================
// Role Checking Helpers
// =============================================================================

/**
 * Check if the current user has a specific role.
 *
 * @param role - The role name to check
 * @returns true if the user has the role
 * @example
 * if (hasRole("admin")) {
 *   return allow();
 * }
 */
declare function hasRole(role: string): boolean;

/**
 * Check if the current user has any of the specified roles.
 *
 * @param roles - Role names to check
 * @returns true if the user has at least one of the roles
 * @example
 * if (hasAnyRole("admin", "practitioner", "nurse")) {
 *   return allow();
 * }
 */
declare function hasAnyRole(...roles: string[]): boolean;

// =============================================================================
// User Type Helpers
// =============================================================================

/**
 * Check if the user's FHIR type is Patient.
 *
 * @returns true if the user is logged in as a Patient
 * @example
 * if (isPatientUser()) {
 *   // Only allow access to own data
 *   return inPatientCompartment() ? allow() : deny("Access to own data only");
 * }
 */
declare function isPatientUser(): boolean;

/**
 * Check if the user's FHIR type is Practitioner.
 *
 * @returns true if the user is logged in as a Practitioner
 */
declare function isPractitionerUser(): boolean;

// =============================================================================
// Context Helpers
// =============================================================================

/**
 * Get the SMART launch patient context (patient ID from launch).
 *
 * @returns Patient ID if in patient context, null otherwise
 */
declare function getPatientContext(): string | null;

/**
 * Get the SMART launch encounter context.
 *
 * @returns Encounter ID if in encounter context, null otherwise
 */
declare function getEncounterContext(): string | null;

/**
 * Check if the request is within the patient compartment.
 * Useful for patient-scoped access control.
 *
 * @returns true if the resource belongs to the patient in context
 * @example
 * if (isPatientUser() && !inPatientCompartment()) {
 *   return deny("Can only access resources in your patient compartment");
 * }
 */
declare function inPatientCompartment(): boolean;

// =============================================================================
// Console (Logging)
// =============================================================================

declare const console: {
	/**
	 * Log a debug message (traced to server logs).
	 */
	log(message: string): void;
	/**
	 * Log a warning message.
	 */
	warn(message: string): void;
	/**
	 * Log an error message.
	 */
	error(message: string): void;
};

// =============================================================================
// Context Variables
// =============================================================================

/**
 * Current user identity. Null for client credentials flow (no user).
 */
declare const user: {
	/** Internal user ID */
	id: string;
	/** FHIR user reference (e.g., "Practitioner/123") */
	fhirUser?: string;
	/** User's FHIR resource type (e.g., "Practitioner", "Patient") */
	fhirUserType?: string;
	/** User's FHIR resource ID */
	fhirUserId?: string;
	/** Assigned roles */
	roles: string[];
	/** Custom attributes from IdP or user record */
	attributes: Record<string, unknown>;
} | null;

/**
 * OAuth client making the request.
 */
declare const client: {
	/** Client ID */
	id: string;
	/** Client display name */
	name: string;
	/** Whether the client is trusted (confidential + system scopes) */
	trusted: boolean;
	/** Client type: "public", "confidentialSymmetric", "confidentialAsymmetric" */
	clientType: "public" | "confidentialSymmetric" | "confidentialAsymmetric";
};

/**
 * Parsed SMART scopes.
 */
declare const scopes: {
	/** Original scope string */
	raw: string;
	/** Patient-context scopes (e.g., "patient/Observation.rs") */
	patientScopes: string[];
	/** User-context scopes (e.g., "user/Patient.r") */
	userScopes: string[];
	/** System-context scopes (e.g., "system/*.cruds") */
	systemScopes: string[];
	/** Whether any scope grants wildcard (*) resource access */
	hasWildcard: boolean;
	/** Whether launch scope is present */
	launch: boolean;
	/** Whether openid scope is present */
	openid: boolean;
	/** Whether fhirUser scope is present */
	fhirUser: boolean;
	/** Whether offline_access scope is present */
	offlineAccess: boolean;
};

/**
 * Information about the current FHIR request.
 */
declare const request: {
	/** FHIR operation type (e.g., "read", "search", "create") */
	operation: string;
	/** Resource type being accessed */
	resourceType: string;
	/** Resource ID (for instance operations) */
	resourceId?: string;
	/** Compartment type from URL (e.g., "Patient" in /Patient/123/Observation) */
	compartmentType?: string;
	/** Compartment ID from URL */
	compartmentId?: string;
	/** Request body (for create/update) */
	body?: unknown;
	/** Query parameters */
	queryParams: Record<string, string>;
	/** Request path */
	path: string;
	/** HTTP method */
	method: string;
	/** Operation ID for policy targeting (e.g., "fhir.read") */
	operationId?: string;
};

/**
 * The resource being accessed (for read/update/delete operations).
 * Null for create/search operations.
 */
declare const resource: {
	/** The actual FHIR resource JSON */
	resource: unknown;
	/** Resource ID */
	id: string;
	/** Resource type */
	resourceType: string;
	/** Resource version ID */
	versionId?: string;
	/** Last updated timestamp */
	lastUpdated?: string;
	/** Extracted subject reference (Patient, etc.) */
	subject?: string;
	/** Extracted author reference */
	author?: string;
} | null;

/**
 * Environment information.
 */
declare const environment: {
	/** Current server time (ISO 8601) */
	requestTime: string;
	/** Source IP address */
	sourceIp?: string;
	/** Request ID for tracing */
	requestId: string;
	/** SMART patient context (patient ID from launch) */
	patientContext?: string;
	/** SMART encounter context */
	encounterContext?: string;
};
`;

// Flag to track if we've registered types
let typesRegistered = false;

/**
 * React wrapper for Monaco JavaScript Editor for Policy Scripts.
 * Provides syntax highlighting, autocomplete for built-in functions,
 * and context variable documentation.
 */
export function PolicyScriptEditor({
	value = "",
	onChange,
	height = 200,
	readOnly = false,
	className,
	placeholder,
}: PolicyScriptEditorProps) {
	const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
	const monacoRef = useRef<typeof Monaco | null>(null);
	const { colorScheme } = useMantineColorScheme();
	const editorTheme = colorScheme === "dark" ? "vs-dark" : "vs";

	// Setup Monaco when editor mounts
	const handleEditorDidMount: OnMount = useCallback((editor, monaco) => {
		editorRef.current = editor;
		monacoRef.current = monaco;

		// Register type definitions for autocomplete (only once)
		if (!typesRegistered) {
			monaco.languages.typescript.javascriptDefaults.addExtraLib(
				POLICY_SCRIPT_TYPES,
				"policy-script-globals.d.ts",
			);
			typesRegistered = true;
		}

		// Focus the editor
		editor.focus();
	}, []);

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
				language="javascript"
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
					...(placeholder && !value
						? {
								// Show placeholder via CSS when empty
							}
						: {}),
				}}
			/>
		</div>
	);
}

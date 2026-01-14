/**
 * Bot Editor Monaco Configuration
 *
 * Provides TypeScript type definitions and autocomplete for bot scripts.
 * Bot scripts have access to: event, fhir, http, and console globals.
 */

import type * as Monaco from "monaco-editor";

/**
 * Type definitions for bot runtime environment.
 * These provide autocomplete for global variables available in bot scripts.
 */
const BOT_RUNTIME_TYPES = `
// =============================================================================
// Bot Runtime API Type Definitions
// =============================================================================
// These types provide autocomplete support for the bot scripting environment.
// Bots have access to global variables: event, fhir, http, and console.

declare global {
  /** The event that triggered this bot execution */
  const event: BotEvent;

  /** FHIR client for resource operations */
  const fhir: FhirClient;

  /** HTTP client for making external requests */
  const http: HttpClient;

  /** Console for logging (output appears in bot execution logs) */
  const console: BotConsole;
}

// ============================================================================
// Event Types
// ============================================================================

/**
 * Event that triggered the bot execution
 */
interface BotEvent {
  /**
   * Type of event that triggered the bot
   * - "created": Resource was created
   * - "updated": Resource was updated
   * - "deleted": Resource was deleted
   * - "manual": Manual/test execution
   */
  type: "created" | "updated" | "deleted" | "manual";

  /** The FHIR resource that triggered the event */
  resource: FhirResource;

  /** Previous version of the resource (only for "updated" events) */
  previous?: FhirResource;

  /** ISO 8601 timestamp when the event occurred */
  timestamp: string;

  /** Resource type (e.g., "Patient", "Observation") */
  resourceType: string;
}

// ============================================================================
// FHIR Client
// ============================================================================

/**
 * FHIR client for performing CRUD operations on resources
 */
interface FhirClient {
  /**
   * Create a new FHIR resource
   * @param resource - The resource to create (must include resourceType)
   * @returns The created resource with id and meta populated
   * @example
   * const task = fhir.create({
   *   resourceType: 'Task',
   *   status: 'requested',
   *   intent: 'order',
   *   description: 'Follow up with patient'
   * });
   */
  create<T extends FhirResource>(resource: T): T;

  /**
   * Read a FHIR resource by type and id
   * @param resourceType - The type of resource (e.g., "Patient")
   * @param id - The resource id
   * @returns The resource or throws if not found
   * @example
   * const patient = fhir.read('Patient', '123');
   */
  read<T extends FhirResource>(resourceType: string, id: string): T;

  /**
   * Update an existing FHIR resource
   * @param resource - The resource to update (must include resourceType and id)
   * @returns The updated resource
   * @example
   * patient.active = false;
   * const updated = fhir.update(patient);
   */
  update<T extends FhirResource>(resource: T): T;

  /**
   * Delete a FHIR resource
   * @param resourceType - The type of resource
   * @param id - The resource id
   * @example
   * fhir.delete('Task', '456');
   */
  delete(resourceType: string, id: string): void;

  /**
   * Search for FHIR resources
   * @param resourceType - The type of resource to search
   * @param params - Search parameters as key-value pairs
   * @returns A Bundle containing matching resources
   * @example
   * const results = fhir.search('Observation', {
   *   patient: 'Patient/123',
   *   code: '85354-9',
   *   _sort: '-date',
   *   _count: '10'
   * });
   */
  search(resourceType: string, params?: Record<string, string>): Bundle;

  /**
   * Patch a FHIR resource with partial updates
   * @param resourceType - The type of resource
   * @param id - The resource id
   * @param patch - Partial resource with fields to update
   * @returns The updated resource
   * @example
   * const updated = fhir.patch('Patient', '123', { active: false });
   */
  patch<T extends FhirResource>(
    resourceType: string,
    id: string,
    patch: Partial<T>
  ): T;

  /**
   * Execute a FHIR operation
   * @param operation - Operation name (e.g., "$validate")
   * @param params - Operation parameters
   * @returns Operation result
   */
  operation(
    operation: string,
    params?: Record<string, unknown>
  ): FhirResource | Bundle;
}

// ============================================================================
// HTTP Client
// ============================================================================

/**
 * HTTP client for making external requests
 */
interface HttpClient {
  /**
   * Make an HTTP request to an external URL
   * @param url - The URL to fetch
   * @param options - Request options (method, headers, body, timeout)
   * @returns Response with status, headers, and body
   * @example
   * // GET request
   * const response = http.fetch('https://api.example.com/data');
   *
   * // POST request with JSON body
   * const response = http.fetch('https://api.example.com/webhook', {
   *   method: 'POST',
   *   headers: { 'Content-Type': 'application/json' },
   *   body: { message: 'Hello' }
   * });
   */
  fetch(url: string, options?: FetchOptions): FetchResponse;
}

/**
 * Options for HTTP fetch requests
 */
interface FetchOptions {
  /** HTTP method (default: GET) */
  method?: "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS";

  /** Request headers */
  headers?: Record<string, string>;

  /** Request body (will be JSON-serialized if object) */
  body?: unknown;

  /** Request timeout in milliseconds (default: 30000) */
  timeout?: number;
}

/**
 * HTTP fetch response
 */
interface FetchResponse {
  /** Whether the response status is 2xx */
  ok: boolean;

  /** HTTP status code */
  status: number;

  /** HTTP status text */
  statusText: string;

  /** Response headers */
  headers: Record<string, string>;

  /** Response body (automatically parsed if JSON) */
  body: unknown;

  /** Final URL after redirects */
  url: string;
}

// ============================================================================
// Console
// ============================================================================

/**
 * Console for logging messages during bot execution.
 * All output is captured and available in execution logs.
 */
interface BotConsole {
  /** Log an informational message */
  log(...args: unknown[]): void;

  /** Log an info-level message */
  info(...args: unknown[]): void;

  /** Log a debug-level message */
  debug(...args: unknown[]): void;

  /** Log a warning message */
  warn(...args: unknown[]): void;

  /** Log an error message */
  error(...args: unknown[]): void;
}

// ============================================================================
// FHIR Base Types
// ============================================================================

/**
 * Base interface for all FHIR resources
 */
interface FhirResource {
  /** Resource type (e.g., "Patient", "Observation") */
  resourceType: string;

  /** Logical id of this resource */
  id?: string;

  /** Metadata about the resource */
  meta?: Meta;

  /** A set of rules under which this content was created */
  implicitRules?: string;

  /** Language of the resource content */
  language?: string;
}

/**
 * Resource metadata
 */
interface Meta {
  /** Version specific identifier */
  versionId?: string;

  /** When the resource version last changed */
  lastUpdated?: string;

  /** Identifies where the resource comes from */
  source?: string;

  /** Profiles this resource claims to conform to */
  profile?: string[];

  /** Security Labels applied to this resource */
  security?: Coding[];

  /** Tags applied to this resource */
  tag?: Coding[];
}

/**
 * A reference to a code defined by a terminology system
 */
interface Coding {
  /** Identity of the terminology system */
  system?: string;

  /** Version of the system */
  version?: string;

  /** Symbol in syntax defined by the system */
  code?: string;

  /** Representation defined by the system */
  display?: string;

  /** If this coding was chosen directly by the user */
  userSelected?: boolean;
}

/**
 * A reference from one resource to another
 */
interface Reference {
  /** Literal reference, Relative, internal or absolute URL */
  reference?: string;

  /** Type the reference refers to (e.g., "Patient") */
  type?: string;

  /** Logical reference, when literal reference is not known */
  identifier?: Identifier;

  /** Text alternative for the resource */
  display?: string;
}

/**
 * An identifier - identifies some entity uniquely and unambiguously
 */
interface Identifier {
  /** usual | official | temp | secondary | old */
  use?: "usual" | "official" | "temp" | "secondary" | "old";

  /** Description of identifier */
  type?: CodeableConcept;

  /** The namespace for the identifier value */
  system?: string;

  /** The value that is unique */
  value?: string;

  /** Time period when id is/was valid for use */
  period?: Period;

  /** Organization that issued id (may be just text) */
  assigner?: Reference;
}

/**
 * Concept - reference to a terminology or just text
 */
interface CodeableConcept {
  /** Code defined by a terminology system */
  coding?: Coding[];

  /** Plain text representation of the concept */
  text?: string;
}

/**
 * Time range defined by start and end date/time
 */
interface Period {
  /** Starting time with inclusive boundary */
  start?: string;

  /** End time with inclusive boundary, if not ongoing */
  end?: string;
}

/**
 * A collection of resources
 */
interface Bundle extends FhirResource {
  resourceType: "Bundle";

  /** Persistent identifier for the bundle */
  identifier?: Identifier;

  /** document | message | transaction | transaction-response | batch | batch-response | history | searchset | collection */
  type:
    | "document"
    | "message"
    | "transaction"
    | "transaction-response"
    | "batch"
    | "batch-response"
    | "history"
    | "searchset"
    | "collection";

  /** When the bundle was assembled */
  timestamp?: string;

  /** If search, the total number of matches */
  total?: number;

  /** Links related to this Bundle */
  link?: BundleLink[];

  /** Entry in the bundle */
  entry?: BundleEntry[];
}

interface BundleLink {
  /** See http://www.iana.org/assignments/link-relations/link-relations.xhtml */
  relation: string;

  /** Reference details for the link */
  url: string;
}

interface BundleEntry {
  /** Links related to this entry */
  link?: BundleLink[];

  /** URI for resource (Absolute URL server address or URI for UUID/OID) */
  fullUrl?: string;

  /** A resource in the bundle */
  resource?: FhirResource;

  /** Search related information */
  search?: { mode?: "match" | "include" | "outcome"; score?: number };

  /** Additional execution information (transaction/batch/history) */
  request?: { method: string; url: string };

  /** Results of execution (transaction/batch/history) */
  response?: { status: string; location?: string; etag?: string };
}

// ============================================================================
// Common FHIR Resource Types
// ============================================================================

/**
 * Patient resource
 */
interface Patient extends FhirResource {
  resourceType: "Patient";
  identifier?: Identifier[];
  active?: boolean;
  name?: HumanName[];
  telecom?: ContactPoint[];
  gender?: "male" | "female" | "other" | "unknown";
  birthDate?: string;
  deceasedBoolean?: boolean;
  deceasedDateTime?: string;
  address?: Address[];
  maritalStatus?: CodeableConcept;
  multipleBirthBoolean?: boolean;
  multipleBirthInteger?: number;
  photo?: Attachment[];
  contact?: PatientContact[];
  communication?: PatientCommunication[];
  generalPractitioner?: Reference[];
  managingOrganization?: Reference;
  link?: PatientLink[];
}

interface HumanName {
  use?: "usual" | "official" | "temp" | "nickname" | "anonymous" | "old" | "maiden";
  text?: string;
  family?: string;
  given?: string[];
  prefix?: string[];
  suffix?: string[];
  period?: Period;
}

interface ContactPoint {
  system?: "phone" | "fax" | "email" | "pager" | "url" | "sms" | "other";
  value?: string;
  use?: "home" | "work" | "temp" | "old" | "mobile";
  rank?: number;
  period?: Period;
}

interface Address {
  use?: "home" | "work" | "temp" | "old" | "billing";
  type?: "postal" | "physical" | "both";
  text?: string;
  line?: string[];
  city?: string;
  district?: string;
  state?: string;
  postalCode?: string;
  country?: string;
  period?: Period;
}

interface Attachment {
  contentType?: string;
  language?: string;
  data?: string;
  url?: string;
  size?: number;
  hash?: string;
  title?: string;
  creation?: string;
}

interface PatientContact {
  relationship?: CodeableConcept[];
  name?: HumanName;
  telecom?: ContactPoint[];
  address?: Address;
  gender?: "male" | "female" | "other" | "unknown";
  organization?: Reference;
  period?: Period;
}

interface PatientCommunication {
  language: CodeableConcept;
  preferred?: boolean;
}

interface PatientLink {
  other: Reference;
  type: "replaced-by" | "replaces" | "refer" | "seealso";
}

/**
 * Observation resource
 */
interface Observation extends FhirResource {
  resourceType: "Observation";
  identifier?: Identifier[];
  basedOn?: Reference[];
  partOf?: Reference[];
  status: "registered" | "preliminary" | "final" | "amended" | "corrected" | "cancelled" | "entered-in-error" | "unknown";
  category?: CodeableConcept[];
  code: CodeableConcept;
  subject?: Reference;
  focus?: Reference[];
  encounter?: Reference;
  effectiveDateTime?: string;
  effectivePeriod?: Period;
  issued?: string;
  performer?: Reference[];
  valueQuantity?: Quantity;
  valueCodeableConcept?: CodeableConcept;
  valueString?: string;
  valueBoolean?: boolean;
  valueInteger?: number;
  valueRange?: Range;
  valueRatio?: Ratio;
  dataAbsentReason?: CodeableConcept;
  interpretation?: CodeableConcept[];
  note?: Annotation[];
  bodySite?: CodeableConcept;
  method?: CodeableConcept;
  specimen?: Reference;
  device?: Reference;
  referenceRange?: ObservationReferenceRange[];
  hasMember?: Reference[];
  derivedFrom?: Reference[];
  component?: ObservationComponent[];
}

interface Quantity {
  value?: number;
  comparator?: "<" | "<=" | ">=" | ">";
  unit?: string;
  system?: string;
  code?: string;
}

interface Range {
  low?: Quantity;
  high?: Quantity;
}

interface Ratio {
  numerator?: Quantity;
  denominator?: Quantity;
}

interface Annotation {
  authorReference?: Reference;
  authorString?: string;
  time?: string;
  text: string;
}

interface ObservationReferenceRange {
  low?: Quantity;
  high?: Quantity;
  type?: CodeableConcept;
  appliesTo?: CodeableConcept[];
  age?: Range;
  text?: string;
}

interface ObservationComponent {
  code: CodeableConcept;
  valueQuantity?: Quantity;
  valueCodeableConcept?: CodeableConcept;
  valueString?: string;
  valueBoolean?: boolean;
  valueInteger?: number;
  valueRange?: Range;
  valueRatio?: Ratio;
  dataAbsentReason?: CodeableConcept;
  interpretation?: CodeableConcept[];
  referenceRange?: ObservationReferenceRange[];
}

/**
 * Task resource
 */
interface Task extends FhirResource {
  resourceType: "Task";
  identifier?: Identifier[];
  instantiatesCanonical?: string;
  instantiatesUri?: string;
  basedOn?: Reference[];
  groupIdentifier?: Identifier;
  partOf?: Reference[];
  status: "draft" | "requested" | "received" | "accepted" | "rejected" | "ready" | "cancelled" | "in-progress" | "on-hold" | "failed" | "completed" | "entered-in-error";
  statusReason?: CodeableConcept;
  businessStatus?: CodeableConcept;
  intent: "unknown" | "proposal" | "plan" | "order" | "original-order" | "reflex-order" | "filler-order" | "instance-order" | "option";
  priority?: "routine" | "urgent" | "asap" | "stat";
  code?: CodeableConcept;
  description?: string;
  focus?: Reference;
  for?: Reference;
  encounter?: Reference;
  executionPeriod?: Period;
  authoredOn?: string;
  lastModified?: string;
  requester?: Reference;
  performerType?: CodeableConcept[];
  owner?: Reference;
  location?: Reference;
  reasonCode?: CodeableConcept;
  reasonReference?: Reference;
  insurance?: Reference[];
  note?: Annotation[];
  relevantHistory?: Reference[];
  input?: TaskInput[];
  output?: TaskOutput[];
}

interface TaskInput {
  type: CodeableConcept;
  valueString?: string;
  valueInteger?: number;
  valueBoolean?: boolean;
  valueReference?: Reference;
  valueCodeableConcept?: CodeableConcept;
  [key: string]: unknown;
}

interface TaskOutput {
  type: CodeableConcept;
  valueString?: string;
  valueInteger?: number;
  valueBoolean?: boolean;
  valueReference?: Reference;
  valueCodeableConcept?: CodeableConcept;
  [key: string]: unknown;
}

/**
 * Encounter resource
 */
interface Encounter extends FhirResource {
  resourceType: "Encounter";
  identifier?: Identifier[];
  status: "planned" | "arrived" | "triaged" | "in-progress" | "onleave" | "finished" | "cancelled" | "entered-in-error" | "unknown";
  statusHistory?: EncounterStatusHistory[];
  class: Coding;
  classHistory?: EncounterClassHistory[];
  type?: CodeableConcept[];
  serviceType?: CodeableConcept;
  priority?: CodeableConcept;
  subject?: Reference;
  episodeOfCare?: Reference[];
  basedOn?: Reference[];
  participant?: EncounterParticipant[];
  appointment?: Reference[];
  period?: Period;
  length?: Duration;
  reasonCode?: CodeableConcept[];
  reasonReference?: Reference[];
  diagnosis?: EncounterDiagnosis[];
  account?: Reference[];
  hospitalization?: EncounterHospitalization;
  location?: EncounterLocation[];
  serviceProvider?: Reference;
  partOf?: Reference;
}

interface EncounterStatusHistory {
  status: string;
  period: Period;
}

interface EncounterClassHistory {
  class: Coding;
  period: Period;
}

interface EncounterParticipant {
  type?: CodeableConcept[];
  period?: Period;
  individual?: Reference;
}

interface EncounterDiagnosis {
  condition: Reference;
  use?: CodeableConcept;
  rank?: number;
}

interface EncounterHospitalization {
  preAdmissionIdentifier?: Identifier;
  origin?: Reference;
  admitSource?: CodeableConcept;
  reAdmission?: CodeableConcept;
  dietPreference?: CodeableConcept[];
  specialCourtesy?: CodeableConcept[];
  specialArrangement?: CodeableConcept[];
  destination?: Reference;
  dischargeDisposition?: CodeableConcept;
}

interface EncounterLocation {
  location: Reference;
  status?: "planned" | "active" | "reserved" | "completed";
  physicalType?: CodeableConcept;
  period?: Period;
}

interface Duration extends Quantity {}

/**
 * Condition resource
 */
interface Condition extends FhirResource {
  resourceType: "Condition";
  identifier?: Identifier[];
  clinicalStatus?: CodeableConcept;
  verificationStatus?: CodeableConcept;
  category?: CodeableConcept[];
  severity?: CodeableConcept;
  code?: CodeableConcept;
  bodySite?: CodeableConcept[];
  subject: Reference;
  encounter?: Reference;
  onsetDateTime?: string;
  onsetAge?: Quantity;
  onsetPeriod?: Period;
  onsetRange?: Range;
  onsetString?: string;
  abatementDateTime?: string;
  abatementAge?: Quantity;
  abatementPeriod?: Period;
  abatementRange?: Range;
  abatementString?: string;
  recordedDate?: string;
  recorder?: Reference;
  asserter?: Reference;
  stage?: ConditionStage[];
  evidence?: ConditionEvidence[];
  note?: Annotation[];
}

interface ConditionStage {
  summary?: CodeableConcept;
  assessment?: Reference[];
  type?: CodeableConcept;
}

interface ConditionEvidence {
  code?: CodeableConcept[];
  detail?: Reference[];
}

/**
 * Procedure resource
 */
interface Procedure extends FhirResource {
  resourceType: "Procedure";
  identifier?: Identifier[];
  instantiatesCanonical?: string[];
  instantiatesUri?: string[];
  basedOn?: Reference[];
  partOf?: Reference[];
  status: "preparation" | "in-progress" | "not-done" | "on-hold" | "stopped" | "completed" | "entered-in-error" | "unknown";
  statusReason?: CodeableConcept;
  category?: CodeableConcept;
  code?: CodeableConcept;
  subject: Reference;
  encounter?: Reference;
  performedDateTime?: string;
  performedPeriod?: Period;
  performedString?: string;
  performedAge?: Quantity;
  performedRange?: Range;
  recorder?: Reference;
  asserter?: Reference;
  performer?: ProcedurePerformer[];
  location?: Reference;
  reasonCode?: CodeableConcept[];
  reasonReference?: Reference[];
  bodySite?: CodeableConcept[];
  outcome?: CodeableConcept;
  report?: Reference[];
  complication?: CodeableConcept[];
  complicationDetail?: Reference[];
  followUp?: CodeableConcept[];
  note?: Annotation[];
  focalDevice?: ProcedureFocalDevice[];
  usedReference?: Reference[];
  usedCode?: CodeableConcept[];
}

interface ProcedurePerformer {
  function?: CodeableConcept;
  actor: Reference;
  onBehalfOf?: Reference;
}

interface ProcedureFocalDevice {
  action?: CodeableConcept;
  manipulated: Reference;
}
`;

// Flag to track if types have been registered
let botTypesRegistered = false;

/**
 * Configure Monaco editor for bot script editing.
 * Adds type definitions for autocomplete support.
 *
 * @param monaco - Monaco editor instance
 */
export function configureBotEditor(monaco: typeof Monaco): void {
	// Only register types once
	if (botTypesRegistered) {
		return;
	}

	// Add bot runtime types for autocomplete
	monaco.languages.typescript.javascriptDefaults.addExtraLib(
		BOT_RUNTIME_TYPES,
		"bot-runtime-globals.d.ts",
	);

	// Configure compiler options for bot scripts
	monaco.languages.typescript.javascriptDefaults.setCompilerOptions({
		target: monaco.languages.typescript.ScriptTarget.ES2020,
		allowNonTsExtensions: true,
		moduleResolution: monaco.languages.typescript.ModuleResolutionKind.NodeJs,
		module: monaco.languages.typescript.ModuleKind.CommonJS,
		noEmit: true,
		esModuleInterop: true,
		allowJs: true,
		checkJs: true,
		strict: false,
	});

	// Enable semantic validation
	monaco.languages.typescript.javascriptDefaults.setDiagnosticsOptions({
		noSemanticValidation: false,
		noSyntaxValidation: false,
	});

	botTypesRegistered = true;
	console.log("[Monaco] Bot editor types configured");
}

/**
 * Get default bot script template.
 */
export function getDefaultBotScript(): string {
	return `// Bot script - runs when triggered by resource events
// Available globals: event, fhir, http, console

if (event.type === 'created' && event.resource.resourceType === 'Patient') {
  const patient = event.resource;

  // Create a welcome task for new patients
  const task = fhir.create({
    resourceType: 'Task',
    status: 'requested',
    intent: 'order',
    description: \`Welcome patient: \${patient.name?.[0]?.family || 'Unknown'}\`,
    for: { reference: \`Patient/\${patient.id}\` }
  });

  console.log('Created welcome task:', task.id);
  return { taskId: task.id };
}

// Return undefined if no action taken
return undefined;
`;
}

/**
 * Get bot script templates for different use cases.
 */
export const BOT_TEMPLATES = {
	"Welcome Patient": `// Create a welcome task when a new patient is created
if (event.type === 'created' && event.resource.resourceType === 'Patient') {
  const patient = event.resource;

  const task = fhir.create({
    resourceType: 'Task',
    status: 'requested',
    intent: 'order',
    description: \`Welcome patient: \${patient.name?.[0]?.family || 'Unknown'}\`,
    for: { reference: \`Patient/\${patient.id}\` }
  });

  console.log('Created welcome task:', task.id);
  return { taskId: task.id };
}
`,

	"Observation Alert": `// Send alert when critical observation is created
if (event.type === 'created' && event.resource.resourceType === 'Observation') {
  const obs = event.resource;

  // Check for critical values (example: high glucose)
  if (obs.code?.coding?.[0]?.code === '2339-0' &&
      obs.valueQuantity?.value > 200) {

    // Create an alert task
    const alert = fhir.create({
      resourceType: 'Task',
      status: 'requested',
      intent: 'order',
      priority: 'urgent',
      description: \`Critical glucose level: \${obs.valueQuantity.value} mg/dL\`,
      for: obs.subject,
      focus: { reference: \`Observation/\${obs.id}\` }
    });

    console.warn('Created critical alert:', alert.id);
    return { alertId: alert.id };
  }
}
`,

	"Webhook Notification": `// Send webhook when resource is updated
if (event.type === 'updated') {
  const response = http.fetch('https://webhook.example.com/fhir-event', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-FHIR-Event': event.type
    },
    body: {
      resourceType: event.resource.resourceType,
      resourceId: event.resource.id,
      eventType: event.type,
      timestamp: event.timestamp
    }
  });

  if (response.ok) {
    console.log('Webhook sent successfully');
    return { status: 'sent', statusCode: response.status };
  } else {
    console.error('Webhook failed:', response.status, response.statusText);
    return { status: 'failed', error: response.statusText };
  }
}
`,

	"Data Sync": `// Sync patient data to external system
if (event.type === 'created' || event.type === 'updated') {
  const resource = event.resource;

  // Only sync Patient and related resources
  const syncTypes = ['Patient', 'Condition', 'Observation'];
  if (!syncTypes.includes(resource.resourceType)) {
    return;
  }

  const response = http.fetch('https://external-ehr.example.com/api/sync', {
    method: 'PUT',
    headers: {
      'Content-Type': 'application/fhir+json',
      'Authorization': 'Bearer YOUR_API_KEY'
    },
    body: resource,
    timeout: 10000
  });

  console.log(\`Synced \${resource.resourceType}/\${resource.id}: \${response.status}\`);
  return { synced: response.ok, status: response.status };
}
`,

	Empty: `// Bot script
// Available globals: event, fhir, http, console

`,
};

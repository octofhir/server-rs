/**
 * Bot Runtime API Type Definitions
 *
 * These types provide autocomplete support for the bot scripting environment.
 * Bots have access to global variables: event, fhir, http, and console.
 */

// Re-export FHIR types for convenience
export type { FhirResource, Bundle, Patient, Observation, Task } from './fhir';

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
  type: 'created' | 'updated' | 'deleted' | 'manual';

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
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH' | 'HEAD' | 'OPTIONS';

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
  use?: 'usual' | 'official' | 'temp' | 'secondary' | 'old';

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
  resourceType: 'Bundle';

  /** Persistent identifier for the bundle */
  identifier?: Identifier;

  /** document | message | transaction | transaction-response | batch | batch-response | history | searchset | collection */
  type:
    | 'document'
    | 'message'
    | 'transaction'
    | 'transaction-response'
    | 'batch'
    | 'batch-response'
    | 'history'
    | 'searchset'
    | 'collection';

  /** When the bundle was assembled */
  timestamp?: string;

  /** If search, the total number of matches */
  total?: number;

  /** Links related to this Bundle */
  link?: BundleLink[];

  /** Entry in the bundle */
  entry?: BundleEntry[];

  /** Digital Signature */
  signature?: Signature;
}

interface BundleLink {
  /** See http://www.iana.org/assignments/link-relations/link-relations.xhtml#link-relations-1 */
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
  search?: BundleEntrySearch;

  /** Additional execution information (transaction/batch/history) */
  request?: BundleEntryRequest;

  /** Results of execution (transaction/batch/history) */
  response?: BundleEntryResponse;
}

interface BundleEntrySearch {
  /** match | include | outcome */
  mode?: 'match' | 'include' | 'outcome';

  /** Search ranking (between 0 and 1) */
  score?: number;
}

interface BundleEntryRequest {
  /** GET | HEAD | POST | PUT | DELETE | PATCH */
  method: 'GET' | 'HEAD' | 'POST' | 'PUT' | 'DELETE' | 'PATCH';

  /** URL for HTTP equivalent of this entry */
  url: string;

  /** For managing cache currency */
  ifNoneMatch?: string;

  /** For managing cache currency */
  ifModifiedSince?: string;

  /** For managing update contention */
  ifMatch?: string;

  /** For conditional creates */
  ifNoneExist?: string;
}

interface BundleEntryResponse {
  /** Status response code (text optional) */
  status: string;

  /** The location (if the operation returns a location) */
  location?: string;

  /** The Etag for the resource (if relevant) */
  etag?: string;

  /** Server's date time modified */
  lastModified?: string;

  /** OperationOutcome with hints and warnings (for batch/transaction) */
  outcome?: FhirResource;
}

interface Signature {
  /** Indication of the reason the entity signed the object(s) */
  type: Coding[];

  /** When the signature was created */
  when: string;

  /** Who signed */
  who: Reference;

  /** The party represented */
  onBehalfOf?: Reference;

  /** The technical format of the signed resources */
  targetFormat?: string;

  /** The technical format of the signature */
  sigFormat?: string;

  /** The actual signature content (XML DigSig. JWS, picture, etc.) */
  data?: string;
}

// ============================================================================
// Common FHIR Resource Types (for better autocomplete)
// ============================================================================

/**
 * Patient resource
 */
interface Patient extends FhirResource {
  resourceType: 'Patient';
  identifier?: Identifier[];
  active?: boolean;
  name?: HumanName[];
  telecom?: ContactPoint[];
  gender?: 'male' | 'female' | 'other' | 'unknown';
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
  use?: 'usual' | 'official' | 'temp' | 'nickname' | 'anonymous' | 'old' | 'maiden';
  text?: string;
  family?: string;
  given?: string[];
  prefix?: string[];
  suffix?: string[];
  period?: Period;
}

interface ContactPoint {
  system?: 'phone' | 'fax' | 'email' | 'pager' | 'url' | 'sms' | 'other';
  value?: string;
  use?: 'home' | 'work' | 'temp' | 'old' | 'mobile';
  rank?: number;
  period?: Period;
}

interface Address {
  use?: 'home' | 'work' | 'temp' | 'old' | 'billing';
  type?: 'postal' | 'physical' | 'both';
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
  gender?: 'male' | 'female' | 'other' | 'unknown';
  organization?: Reference;
  period?: Period;
}

interface PatientCommunication {
  language: CodeableConcept;
  preferred?: boolean;
}

interface PatientLink {
  other: Reference;
  type: 'replaced-by' | 'replaces' | 'refer' | 'seealso';
}

/**
 * Observation resource
 */
interface Observation extends FhirResource {
  resourceType: 'Observation';
  identifier?: Identifier[];
  basedOn?: Reference[];
  partOf?: Reference[];
  status: 'registered' | 'preliminary' | 'final' | 'amended' | 'corrected' | 'cancelled' | 'entered-in-error' | 'unknown';
  category?: CodeableConcept[];
  code: CodeableConcept;
  subject?: Reference;
  focus?: Reference[];
  encounter?: Reference;
  effectiveDateTime?: string;
  effectivePeriod?: Period;
  effectiveTiming?: Timing;
  effectiveInstant?: string;
  issued?: string;
  performer?: Reference[];
  valueQuantity?: Quantity;
  valueCodeableConcept?: CodeableConcept;
  valueString?: string;
  valueBoolean?: boolean;
  valueInteger?: number;
  valueRange?: Range;
  valueRatio?: Ratio;
  valueSampledData?: SampledData;
  valueTime?: string;
  valueDateTime?: string;
  valuePeriod?: Period;
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
  comparator?: '<' | '<=' | '>=' | '>';
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

interface Timing {
  event?: string[];
  repeat?: TimingRepeat;
  code?: CodeableConcept;
}

interface TimingRepeat {
  boundsDuration?: Duration;
  boundsRange?: Range;
  boundsPeriod?: Period;
  count?: number;
  countMax?: number;
  duration?: number;
  durationMax?: number;
  durationUnit?: 's' | 'min' | 'h' | 'd' | 'wk' | 'mo' | 'a';
  frequency?: number;
  frequencyMax?: number;
  period?: number;
  periodMax?: number;
  periodUnit?: 's' | 'min' | 'h' | 'd' | 'wk' | 'mo' | 'a';
  dayOfWeek?: ('mon' | 'tue' | 'wed' | 'thu' | 'fri' | 'sat' | 'sun')[];
  timeOfDay?: string[];
  when?: string[];
  offset?: number;
}

interface Duration extends Quantity {}

interface SampledData {
  origin: Quantity;
  period: number;
  factor?: number;
  lowerLimit?: number;
  upperLimit?: number;
  dimensions: number;
  data?: string;
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
  valueSampledData?: SampledData;
  valueTime?: string;
  valueDateTime?: string;
  valuePeriod?: Period;
  dataAbsentReason?: CodeableConcept;
  interpretation?: CodeableConcept[];
  referenceRange?: ObservationReferenceRange[];
}

/**
 * Task resource
 */
interface Task extends FhirResource {
  resourceType: 'Task';
  identifier?: Identifier[];
  instantiatesCanonical?: string;
  instantiatesUri?: string;
  basedOn?: Reference[];
  groupIdentifier?: Identifier;
  partOf?: Reference[];
  status: 'draft' | 'requested' | 'received' | 'accepted' | 'rejected' | 'ready' | 'cancelled' | 'in-progress' | 'on-hold' | 'failed' | 'completed' | 'entered-in-error';
  statusReason?: CodeableConcept;
  businessStatus?: CodeableConcept;
  intent: 'unknown' | 'proposal' | 'plan' | 'order' | 'original-order' | 'reflex-order' | 'filler-order' | 'instance-order' | 'option';
  priority?: 'routine' | 'urgent' | 'asap' | 'stat';
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
  restriction?: TaskRestriction;
  input?: TaskInput[];
  output?: TaskOutput[];
}

interface TaskRestriction {
  repetitions?: number;
  period?: Period;
  recipient?: Reference[];
}

interface TaskInput {
  type: CodeableConcept;
  valueBase64Binary?: string;
  valueBoolean?: boolean;
  valueCanonical?: string;
  valueCode?: string;
  valueDate?: string;
  valueDateTime?: string;
  valueDecimal?: number;
  valueId?: string;
  valueInstant?: string;
  valueInteger?: number;
  valueMarkdown?: string;
  valueOid?: string;
  valuePositiveInt?: number;
  valueString?: string;
  valueTime?: string;
  valueUnsignedInt?: number;
  valueUri?: string;
  valueUrl?: string;
  valueUuid?: string;
  valueAddress?: Address;
  valueAge?: Quantity;
  valueAnnotation?: Annotation;
  valueAttachment?: Attachment;
  valueCodeableConcept?: CodeableConcept;
  valueCoding?: Coding;
  valueContactPoint?: ContactPoint;
  valueCount?: Quantity;
  valueDistance?: Quantity;
  valueDuration?: Duration;
  valueHumanName?: HumanName;
  valueIdentifier?: Identifier;
  valueMoney?: Money;
  valuePeriod?: Period;
  valueQuantity?: Quantity;
  valueRange?: Range;
  valueRatio?: Ratio;
  valueReference?: Reference;
  valueSampledData?: SampledData;
  valueSignature?: Signature;
  valueTiming?: Timing;
}

interface TaskOutput {
  type: CodeableConcept;
  valueBase64Binary?: string;
  valueBoolean?: boolean;
  valueCanonical?: string;
  valueCode?: string;
  valueDate?: string;
  valueDateTime?: string;
  valueDecimal?: number;
  valueId?: string;
  valueInstant?: string;
  valueInteger?: number;
  valueMarkdown?: string;
  valueOid?: string;
  valuePositiveInt?: number;
  valueString?: string;
  valueTime?: string;
  valueUnsignedInt?: number;
  valueUri?: string;
  valueUrl?: string;
  valueUuid?: string;
  valueAddress?: Address;
  valueAge?: Quantity;
  valueAnnotation?: Annotation;
  valueAttachment?: Attachment;
  valueCodeableConcept?: CodeableConcept;
  valueCoding?: Coding;
  valueContactPoint?: ContactPoint;
  valueCount?: Quantity;
  valueDistance?: Quantity;
  valueDuration?: Duration;
  valueHumanName?: HumanName;
  valueIdentifier?: Identifier;
  valueMoney?: Money;
  valuePeriod?: Period;
  valueQuantity?: Quantity;
  valueRange?: Range;
  valueRatio?: Ratio;
  valueReference?: Reference;
  valueSampledData?: SampledData;
  valueSignature?: Signature;
  valueTiming?: Timing;
}

interface Money {
  value?: number;
  currency?: string;
}

export {};

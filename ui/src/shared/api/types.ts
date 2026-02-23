// Server API types
export interface HealthResponse {
  status: "ok" | "degraded" | "down";
  details?: string;
}

export interface BuildInfo {
  serverVersion: string;
  commit: string;
  commitTimestamp: string;
  uiVersion?: string;
}

export interface ServerFeatures {
  sqlOnFhir: boolean;
  graphql: boolean;
  bulkExport: boolean;
  dbConsole: boolean;
  auth: boolean;
  cql: boolean;
}

export interface ServerSettings {
  fhirVersion: string;
  features: ServerFeatures;
}

// Resource type categorization
export interface CategorizedResourceType {
  name: string;
  category: "fhir" | "system" | "custom";
  url?: string;
  package: string;
}

export interface CategoryCounts {
  all: number;
  fhir: number;
  system: number;
  custom: number;
}

export interface CategorizedResourceTypesResponse {
  types: CategorizedResourceType[];
  counts: CategoryCounts;
}

// FHIR types (minimal)
export interface FhirResource {
  resourceType: string;
  id?: string;
  meta?: {
    versionId?: string;
    lastUpdated?: string;
  };
  [key: string]: any;
}

export interface FhirBundle {
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
  }>;
}

export interface FhirOperationOutcome {
  resourceType: "OperationOutcome";
  issue: Array<{
    severity: "fatal" | "error" | "warning" | "information";
    code: string;
    diagnostics?: string;
    location?: string[];
  }>;
}

// GraphQL types
export interface GraphQLError {
  message: string;
  locations?: Array<{ line: number; column: number }>;
  path?: Array<string | number>;
  extensions?: Record<string, unknown>;
}

export interface GraphQLResponse {
  data?: Record<string, unknown> | null;
  errors?: GraphQLError[];
  extensions?: Record<string, unknown>;
}

// SQL execution types
export interface SqlRequest {
  query: string;
  /** Optional bind parameters for parameterized queries ($1, $2, etc.) */
  params?: SqlValue[];
}

export type SqlValue = string | number | boolean | null | Record<string, unknown>;

export interface SqlResponse {
  columns: string[];
  rows: SqlValue[][];
  rowCount: number;
  executionTimeMs: number;
}

// Auth types
export interface LoginRequest {
  grant_type: "password";
  client_id: string;
  username: string;
  password: string;
}

export interface TokenResponse {
  access_token: string;
  token_type: "Bearer";
  expires_in: number;
  refresh_token?: string; // Optional: used for refreshing access tokens
  scope?: string;
}

export interface UserInfo {
  sub: string;
  name?: string;
  preferred_username?: string;
  email?: string;
  fhirUser?: string;
  roles?: string[];
}

export interface AuthError {
  error: string;
  error_description?: string;
}

export interface LogoutResponse {
  success: boolean;
  message: string;
}

// User Management types
export interface UserResource extends FhirResource {
  resourceType: "User";
  username: string;
  password?: string;
  email?: string;
  name?: string;
  fhirUser?: Reference;
  active: boolean;
  roles: string[];
  status?: "active" | "inactive" | "locked";
  lastLogin?: string;
  mfaEnabled?: boolean;
  identity: UserIdentityElement[];
  createdAt?: string;
  updatedAt?: string;
}

// User Session types
export interface UserSession {
  id: string;
  userId: string;
  clientId?: string;
  clientName?: string;
  ipAddress?: string;
  userAgent?: string;
  createdAt: string;
  expiresAt: string;
  lastActivity?: string;
  isCurrent?: boolean;
}

// Role Management types
export interface RoleResource extends FhirResource {
  resourceType: "Role";
  name: string;
  description?: string;
  permissions: string[];
  isSystem?: boolean;
  active: boolean;
  createdAt?: string;
  updatedAt?: string;
}

// Permission types
export interface Permission {
  code: string;
  display: string;
  category: string;
  description?: string;
}

// Bundle type for list responses
export interface Bundle<T extends FhirResource = FhirResource> {
  resourceType: "Bundle";
  type: string;
  total?: number;
  link?: Array<{
    relation: string;
    url: string;
  }>;
  entry?: Array<{
    resource: T;
    fullUrl?: string;
  }>;
}

export interface UserIdentityElement {
  provider: Reference;
  externalId: string;
  email?: string;
  linkedAt?: string;
}

export interface Reference {
  reference?: string;
  display?: string;
}

export interface LinkIdentityRequest {
  provider_id: string;
  external_id: string;
  email?: string;
}

export interface UnlinkIdentityRequest {
  provider_id: string;
}

export interface UserSearchParams {
  email?: string;
  username?: string;
  active?: boolean;
  "identity-provider"?: string;
  _count?: number;
  _offset?: number;
}

// Operation types
export interface AppReference {
  id: string;
  name: string;
}

export interface OperationDefinition {
  id: string;
  name: string;
  description?: string;
  category: string;
  methods: string[];
  path_pattern: string;
  public: boolean;
  module: string;
  app?: AppReference;
}

export interface OperationsResponse {
  operations: OperationDefinition[];
  total: number;
}

export interface OperationUpdateRequest {
  public?: boolean;
  description?: string;
}

// HTTP types
export type HttpMethod = "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS";

export interface HttpRequestConfig {
  method: HttpMethod;
  url: string;
  headers?: Record<string, string>;
  data?: any;
  timeout?: number;
  credentials?: RequestCredentials;
}

export interface HttpResponse<T = any> {
  data: T;
  status: number;
  statusText: string;
  headers: Record<string, string>;
  config: HttpRequestConfig;
}

// REST console introspection types (unified v3)
export interface RestConsoleResponse {
  schema_version: number;
  fhir_version: string;
  base_path: string;
  generated_at: string;
  suggestions: RestConsoleSuggestions;
  search_params: Record<string, RestConsoleSearchParam[]>;
  resources: ResourceCapability[];
  system_operations: OperationCapabilityInfo[];
  special_params: SpecialParamInfo[];
}

export interface RestConsoleSuggestions {
  resources: AutocompleteSuggestion[];
  system_operations: AutocompleteSuggestion[];
  type_operations: AutocompleteSuggestion[];
  instance_operations: AutocompleteSuggestion[];
  api_endpoints: AutocompleteSuggestion[];
}

export type SuggestionKind = "resource" | "system-op" | "type-op" | "instance-op" | "api-endpoint";

export interface AutocompleteSuggestion {
  id: string;
  kind: SuggestionKind;
  label: string;
  path_template: string;
  methods: string[];
  placeholders: string[];
  description?: string;
  metadata: SuggestionMetadata;
}

export interface SuggestionMetadata {
  resource_type?: string;
  affects_state: boolean;
  requires_body: boolean;
  category?: string;
}

export interface RestConsoleSearchParam {
  code: string;
  type: string;
  description?: string;
  modifiers: ModifierSuggestion[];
  comparators: string[];
  targets: string[];
  is_common: boolean;
}

export interface ModifierSuggestion {
  code: string;
  description?: string;
}

export interface ResourceCapability {
  resource_type: string;
  search_params: EnrichedSearchParam[];
  includes: IncludeCapability[];
  rev_includes: IncludeCapability[];
  sort_params: string[];
  type_operations: OperationCapabilityInfo[];
  instance_operations: OperationCapabilityInfo[];
}

export interface EnrichedSearchParam {
  code: string;
  param_type: string;
  description?: string;
  modifiers: ModifierSuggestion[];
  comparators: string[];
  targets: string[];
  chains: ChainInfo[];
  is_common: boolean;
}

export interface ChainInfo {
  target_type: string;
  target_params: string[];
}

export interface IncludeCapability {
  param_code: string;
  target_types: string[];
}

export interface OperationCapabilityInfo {
  code: string;
  method: string;
  description?: string;
  affects_state: boolean;
  resource_types: string[];
}

export interface SpecialParamInfo {
  name: string;
  description?: string;
  supported: boolean;
  examples: string[];
}

// Package management types
export interface PackageInfo {
  name: string;
  version: string;
  fhirVersion?: string;
  resourceCount: number;
  installedAt?: string;
}

export interface PackageListResponse {
  packages: PackageInfo[];
  serverFhirVersion: string;
}

export interface ResourceTypeSummary {
  resourceType: string;
  count: number;
}

export interface PackageDetailResponse {
  name: string;
  version: string;
  fhirVersion?: string;
  description?: string;
  resourceCount: number;
  installedAt?: string;
  isCompatible: boolean;
  resourceTypes: ResourceTypeSummary[];
}

export interface PackageResourceSummary {
  id?: string;
  url?: string;
  name?: string;
  version?: string;
  resourceType: string;
}

export interface PackageResourcesResponse {
  resources: PackageResourceSummary[];
  total: number;
}

export interface PackageLookupResponse {
  name: string;
  versions: string[];
  installedVersions: string[];
}

export interface PackageInstallRequest {
  name: string;
  version: string;
}

export interface PackageInstallResponse {
  success: boolean;
  name: string;
  version: string;
  fhirVersion: string;
  resourceCount: number;
  message: string;
}

// Package search types
export interface PackageSearchResult {
  name: string;
  versions: string[];
  description?: string;
  latestVersion: string;
}

export interface PackageSearchResponse {
  query: string;
  packages: PackageSearchResult[];
  total: number;
}

// Package install progress event types (SSE)
export type InstallEventType =
  | "started"
  | "resolving_dependencies"
  | "dependencies_resolved"
  | "download_started"
  | "download_progress"
  | "download_completed"
  | "extracting"
  | "extracted"
  | "indexing"
  | "package_installed"
  | "completed"
  | "error"
  | "skipped";

export interface InstallEventStarted {
  type: "started";
  total_packages: number;
}

export interface InstallEventResolvingDependencies {
  type: "resolving_dependencies";
  package: string;
  version: string;
}

export interface InstallEventDependenciesResolved {
  type: "dependencies_resolved";
  packages: string[];
}

export interface InstallEventDownloadStarted {
  type: "download_started";
  package: string;
  version: string;
  current: number;
  total: number;
  total_bytes?: number;
}

export interface InstallEventDownloadProgress {
  type: "download_progress";
  package: string;
  version: string;
  downloaded_bytes: number;
  total_bytes?: number;
  percent: number;
}

export interface InstallEventDownloadCompleted {
  type: "download_completed";
  package: string;
  version: string;
}

export interface InstallEventExtracting {
  type: "extracting";
  package: string;
  version: string;
  current: number;
  total: number;
}

export interface InstallEventExtracted {
  type: "extracted";
  package: string;
  version: string;
  resource_count: number;
}

export interface InstallEventIndexing {
  type: "indexing";
  package: string;
  version: string;
  current: number;
  total: number;
}

export interface InstallEventPackageInstalled {
  type: "package_installed";
  package: string;
  version: string;
  resource_count: number;
}

export interface InstallEventCompleted {
  type: "completed";
  total_installed: number;
  total_resources: number;
  duration_ms: number;
}

export interface InstallEventError {
  type: "error";
  package?: string;
  version?: string;
  message: string;
}

export interface InstallEventSkipped {
  type: "skipped";
  package: string;
  version: string;
  reason: string;
}

export type InstallEvent =
  | InstallEventStarted
  | InstallEventResolvingDependencies
  | InstallEventDependenciesResolved
  | InstallEventDownloadStarted
  | InstallEventDownloadProgress
  | InstallEventDownloadCompleted
  | InstallEventExtracting
  | InstallEventExtracted
  | InstallEventIndexing
  | InstallEventPackageInstalled
  | InstallEventCompleted
  | InstallEventError
  | InstallEventSkipped;

// System Logs types
export type LogLevel = "trace" | "debug" | "info" | "warn" | "error";

export interface LogEntry {
  id: string;
  timestamp: string;
  level: LogLevel;
  target: string;
  message: string;
  fields?: Record<string, unknown>;
  span?: {
    name: string;
    target: string;
  };
}

export interface LogFilters {
  levels: LogLevel[];
  search?: string;
  target?: string;
  startTime?: string;
  endTime?: string;
}

export interface LogStreamConfig {
  maxEntries?: number;
  filters?: LogFilters;
}

// Audit Trail types - Simplified view of FHIR R4 AuditEvent
// Maps to standard FHIR AuditEvent resource

// FHIR R4 AuditEvent action codes
export type AuditActionCode = "C" | "R" | "U" | "D" | "E"; // Create, Read, Update, Delete, Execute

// Custom action subtypes for more detail
export type AuditAction =
  | "user.login"
  | "user.logout"
  | "user.login_failed"
  | "resource.create"
  | "resource.read"
  | "resource.update"
  | "resource.delete"
  | "resource.search"
  | "policy.evaluate"
  | "client.auth"
  | "client.create"
  | "client.update"
  | "client.delete"
  | "config.change"
  | "system.startup"
  | "system.shutdown";

// FHIR R4 AuditEvent outcome codes: 0=Success, 4=Minor failure, 8=Serious failure, 12=Major failure
export type AuditOutcomeCode = "0" | "4" | "8" | "12";
export type AuditOutcome = "success" | "failure" | "partial";

// Simplified AuditEvent for UI (derived from FHIR AuditEvent)
export interface AuditEvent {
  resourceType: "AuditEvent";
  id: string;
  timestamp: string; // Maps to recorded
  action: AuditAction;
  actionCode?: AuditActionCode; // FHIR action code
  outcome: AuditOutcome;
  outcomeCode?: AuditOutcomeCode; // FHIR outcome code
  outcomeDescription?: string;

  // Actor (who performed the action) - from agent[]
  actor: {
    type: "user" | "client" | "system";
    id?: string;
    name?: string;
    reference?: string;
  };

  // Source (where the request came from) - from source
  source: {
    ipAddress?: string;
    userAgent?: string;
    site?: string;
    observer?: string;
  };

  // Target (what was affected) - from entity[]
  target?: {
    resourceType?: string;
    resourceId?: string;
    reference?: string;
    query?: string;
  };

  // Changes (for updates) - extension or entity.detail
  changes?: {
    before?: Record<string, unknown>;
    after?: Record<string, unknown>;
    diff?: Array<{
      path: string;
      op: "add" | "remove" | "replace";
      oldValue?: unknown;
      newValue?: unknown;
    }>;
  };

  // Additional context - from extension
  context?: {
    requestId?: string;
    sessionId?: string;
    clientId?: string;
    policyId?: string;
    duration?: number;
    [key: string]: unknown;
  };
}

export interface AuditEventFilters {
  _content?: string; // FHIR search
  action?: AuditActionCode; // FHIR action param
  subtype?: string; // Custom action type
  outcome?: AuditOutcomeCode;
  agent?: string; // actor reference
  "agent-type"?: string;
  entity?: string; // resource reference
  "entity-type"?: string;
  date?: string; // ge/le prefixed
  address?: string; // source IP
  _count?: number;
  _offset?: number;
}

// Simplified filters for UI
export interface AuditEventUIFilters {
  search?: string;
  action?: AuditAction[];
  outcome?: AuditOutcome[];
  actorType?: ("user" | "client" | "system")[];
  actorId?: string;
  resourceType?: string;
  resourceId?: string;
  startTime?: string;
  endTime?: string;
  ipAddress?: string;
}

export interface AuditEventListResponse {
  events: AuditEvent[];
  total: number;
  hasMore: boolean;
  nextCursor?: string;
}

export interface AuditAnalytics {
  activityOverTime: Array<{
    timestamp: string;
    count: number;
    breakdown: Partial<Record<AuditAction, number>>;
  }>;
  topUsers: Array<{
    userId: string;
    userName?: string;
    count: number;
  }>;
  topResources: Array<{
    resourceType: string;
    resourceId?: string;
    count: number;
  }>;
  outcomeBreakdown: Partial<Record<AuditOutcome, number>>;
  actionBreakdown: Partial<Record<AuditAction, number>>;
  failedAttempts: Array<{
    action: AuditAction;
    count: number;
    lastAttempt: string;
  }>;
}

// ============ Automation types ============

export type AutomationStatus = "active" | "inactive" | "error";
export type AutomationTriggerType = "resource_event" | "cron" | "manual";
export type AutomationExecutionStatus = "running" | "completed" | "failed";

export interface Automation {
  id: string;
  name: string;
  description?: string;
  source_code: string;
  compiled_code?: string;
  status: AutomationStatus;
  version: number;
  timeout_ms: number;
  created_at: string;
  updated_at: string;
  triggers?: AutomationTrigger[];
  /** Execution statistics (included in list response) */
  execution_stats?: AutomationExecutionStats;
}

export interface AutomationTrigger {
  id: string;
  automation_id: string;
  trigger_type: AutomationTriggerType;
  resource_type?: string;
  event_types?: string[];
  fhirpath_filter?: string;
  cron_expression?: string;
  created_at: string;
}

export interface AutomationExecution {
  id: string;
  automation_id: string;
  trigger_id?: string;
  status: AutomationExecutionStatus;
  input?: unknown;
  output?: unknown;
  logs?: AutomationLogEntry[];
  error?: string;
  started_at: string;
  completed_at?: string;
  duration_ms?: number;
}

export interface AutomationLogEntry {
  level: "log" | "info" | "debug" | "warn" | "error";
  message: string;
  /** Optional structured data attached to the log entry */
  data?: unknown;
  timestamp?: string;
}

/** Execution statistics for an automation (returned in list view) */
export interface AutomationExecutionStats {
  /** Last execution status: "completed", "failed", "running" */
  last_execution_status?: "completed" | "failed" | "running";
  /** When the last execution occurred (ISO 8601) */
  last_execution_at?: string;
  /** Error message from the last failed execution */
  last_error?: string;
  /** Number of failed executions in the last 24 hours */
  failure_count_24h: number;
  /** Number of successful executions in the last 24 hours */
  success_count_24h: number;
}

export interface AutomationSearchParams {
  status?: AutomationStatus;
  name?: string;
  _count?: number;
  _offset?: number;
}

export interface AutomationListResponse {
  automations: Automation[];
  total: number;
}

export interface CreateAutomationRequest {
  name: string;
  description?: string;
  source_code: string;
  timeout_ms?: number;
  triggers?: CreateTriggerRequest[];
}

export interface UpdateAutomationRequest {
  name?: string;
  description?: string;
  source_code?: string;
  timeout_ms?: number;
}

export interface CreateTriggerRequest {
  trigger_type: AutomationTriggerType;
  resource_type?: string;
  event_types?: string[];
  fhirpath_filter?: string;
  cron_expression?: string;
}

export interface ExecuteAutomationRequest {
  resource?: unknown;
  event_type?: string;
}

export interface TestAutomationRequest {
  source_code: string;
  resource?: unknown;
  event_type?: string;
}

export interface ExecuteAutomationResponse {
  execution_id: string;
  success: boolean;
  output?: unknown;
  logs?: AutomationLogEntry[];
  error?: string;
  duration_ms?: number;
}

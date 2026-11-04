import { isRecord } from "@/shared/api/guards";

/**
 * SQL on FHIR SQLQuery / SQLView (a Library profile) as the console edits it,
 * and its conversion to and from the Library resource.
 *
 * Profile (SoF 3.0.0-ballot, StructureDefinition-SQLQuery): `type` is
 * LibraryTypesCodes#sql-query (#sql-view), the SQL is `content` with
 * `contentType: application/sql` and base64 `data` (plain text in the
 * `sql-text` extension), each table it reads is a `relatedArtifact` of type
 * `depends-on` whose `label` is the name in the SQL and `resource` the
 * canonical of a ViewDefinition, SQLView, ValueSet or ConceptMap, and its
 * inputs are `parameter`s (`use: in`) written `:name` in the SQL.
 */

export const LIBRARY_TYPES = "http://hl7.org/fhir/uv/sql-on-fhir/CodeSystem/LibraryTypesCodes";
export const SQL_TEXT_EXTENSION = "http://hl7.org/fhir/uv/sql-on-fhir/StructureDefinition/sql-text";
const SQL_QUERY_PROFILE = "http://hl7.org/fhir/uv/sql-on-fhir/StructureDefinition/SQLQuery";
const SQL_VIEW_PROFILE = "http://hl7.org/fhir/uv/sql-on-fhir/StructureDefinition/SQLView";

export type SqlLibraryKind = "sql-query" | "sql-view";
export type ArtifactStatus = "draft" | "active" | "retired" | "unknown";

export interface SqlDependency {
  /** Table name in the SQL (`relatedArtifact.label`). */
  label: string;
  /** Canonical URL, optionally `|version`. */
  canonical: string;
  _id?: string;
}

export interface SqlParameter {
  name: string;
  /** FHIR type of the value. */
  type: string;
  required: boolean;
  documentation?: string;
  /** Value to bind on a console run (not part of the Library). */
  value?: string;
  _id?: string;
}

export interface SqlQuery {
  id?: string;
  kind: SqlLibraryKind;
  url?: string;
  version?: string;
  name: string;
  title?: string;
  status: ArtifactStatus;
  description?: string;
  sql: string;
  dependencies: SqlDependency[];
  parameters: SqlParameter[];
  /** The Library as loaded: fields the console does not edit survive a save. */
  source?: Record<string, unknown>;
}

/** FHIR types a parameter can have (those with a scalar SQL value). */
export const PARAMETER_TYPES = [
  "string",
  "code",
  "id",
  "uri",
  "url",
  "canonical",
  "uuid",
  "date",
  "dateTime",
  "instant",
  "time",
  "integer",
  "positiveInt",
  "unsignedInt",
  "integer64",
  "decimal",
  "boolean",
] as const;

const uid = () => crypto.randomUUID();

/** A new query; over `view` (a saved ViewDefinition with a url) when there is one. */
export function createEmptySqlQuery(view?: { url?: string; name?: string; resource?: string }): SqlQuery {
  const label = sqlLabel(view?.name || view?.resource || "patient");
  return {
    kind: "sql-query",
    name: "",
    status: "draft",
    sql: `SELECT *\nFROM ${label}`,
    dependencies: [{ label, canonical: view?.url ?? "", _id: uid() }],
    parameters: [],
  };
}

/** A SQL identifier (`[A-Za-z][A-Za-z0-9_]*`) made from a name. */
function sqlLabel(name: string): string {
  const cleaned = name.replace(/[^A-Za-z0-9_]+/g, "_").replace(/^[^A-Za-z]+/, "").toLowerCase();
  return cleaned || "source";
}

export function sqlLibraryKind(resource: unknown): SqlLibraryKind | undefined {
  if (!isRecord(resource) || resource.resourceType !== "Library") return undefined;
  const coding = isRecord(resource.type) && Array.isArray(resource.type.coding) ? resource.type.coding : [];
  for (const entry of coding) {
    if (!isRecord(entry) || entry.system !== LIBRARY_TYPES) continue;
    if (entry.code === "sql-query" || entry.code === "sql-view") return entry.code;
  }
  return undefined;
}

/** Base64 of UTF-8 text (FHIR `Attachment.data`). */
function encodeBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function decodeBase64(data: string): string {
  const bytes = Uint8Array.from(atob(data), (char) => char.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

/** The SQL of a Library: the portable `application/sql` content, else the first one. */
function readSql(content: unknown): string {
  const attachments = Array.isArray(content) ? content.filter(isRecord) : [];
  const portable =
    attachments.find((a) => a.contentType === "application/sql") ??
    attachments.find((a) => typeof a.contentType === "string" && a.contentType.startsWith("application/sql"));
  if (!portable) return "";
  const text = Array.isArray(portable.extension)
    ? portable.extension.find((e) => isRecord(e) && e.url === SQL_TEXT_EXTENSION)
    : undefined;
  if (isRecord(text) && typeof text.valueString === "string") return text.valueString;
  if (typeof portable.data === "string") {
    try {
      return decodeBase64(portable.data);
    } catch {
      return "";
    }
  }
  return "";
}

const text = (value: unknown) => (typeof value === "string" ? value : undefined);

export function queryFromLibrary(library: Record<string, unknown>): SqlQuery {
  const status = text(library.status);
  return {
    id: text(library.id),
    kind: sqlLibraryKind(library) ?? "sql-query",
    url: text(library.url),
    version: text(library.version),
    name: text(library.name) ?? "",
    title: text(library.title),
    status: status === "active" || status === "retired" || status === "unknown" ? status : "draft",
    description: text(library.description),
    sql: readSql(library.content),
    dependencies: (Array.isArray(library.relatedArtifact) ? library.relatedArtifact : [])
      .filter((a): a is Record<string, unknown> => isRecord(a) && a.type === "depends-on")
      .map((a) => ({ label: text(a.label) ?? "", canonical: text(a.resource) ?? "", _id: uid() })),
    parameters: (Array.isArray(library.parameter) ? library.parameter : [])
      .filter(isRecord)
      .map((p) => ({
        name: text(p.name) ?? "",
        type: text(p.type) ?? "string",
        required: typeof p.min === "number" && p.min >= 1,
        documentation: text(p.documentation),
        _id: uid(),
      })),
    source: library,
  };
}

/** The Library resource of `query`, keeping the loaded fields it does not edit. */
export function libraryFromQuery(query: SqlQuery): Record<string, unknown> {
  const base: Record<string, unknown> = { ...query.source };
  // Fields the console owns are rebuilt from the editor state.
  for (const key of ["url", "version", "title", "description", "relatedArtifact", "parameter"]) {
    delete base[key];
  }
  const otherRelated = (Array.isArray(query.source?.relatedArtifact) ? query.source.relatedArtifact : []).filter(
    (a) => isRecord(a) && a.type !== "depends-on"
  );
  const otherContent = (Array.isArray(query.source?.content) ? query.source.content : []).filter(
    (c) => isRecord(c) && c.contentType !== "application/sql"
  );
  const profile = query.kind === "sql-view" ? SQL_VIEW_PROFILE : SQL_QUERY_PROFILE;
  const meta = isRecord(base.meta) ? { ...base.meta } : {};
  const profiles = Array.isArray(meta.profile) ? meta.profile.filter((p) => p !== SQL_QUERY_PROFILE && p !== SQL_VIEW_PROFILE) : [];
  meta.profile = [profile, ...profiles];

  const library: Record<string, unknown> = {
    ...base,
    resourceType: "Library",
    meta,
    ...(query.id && { id: query.id }),
    ...(query.url ? { url: query.url } : {}),
    ...(query.version ? { version: query.version } : {}),
    name: query.name,
    ...(query.title ? { title: query.title } : {}),
    status: query.status,
    type: { coding: [{ system: LIBRARY_TYPES, code: query.kind }] },
    ...(query.description ? { description: query.description } : {}),
    content: [
      {
        contentType: "application/sql",
        extension: [{ url: SQL_TEXT_EXTENSION, valueString: query.sql }],
        data: encodeBase64(query.sql),
      },
      ...otherContent,
    ],
  };
  const related = [
    ...query.dependencies
      .filter((d) => d.label.trim() || d.canonical.trim())
      .map((d) => ({ type: "depends-on", label: d.label.trim(), resource: d.canonical.trim() })),
    ...otherRelated,
  ];
  if (related.length > 0) library.relatedArtifact = related;
  if (query.kind === "sql-query" && query.parameters.length > 0) {
    library.parameter = query.parameters.map((p) => ({
      name: p.name.trim(),
      use: "in",
      ...(p.required ? { min: 1 } : {}),
      max: "1",
      ...(p.documentation ? { documentation: p.documentation } : {}),
      type: p.type,
    }));
  }
  return library;
}

/** `value[x]` of a parameter value as typed in the console. */
function typedValue(parameter: SqlParameter): Record<string, unknown> | undefined {
  const raw = parameter.value?.trim();
  if (!raw) return undefined;
  const field = `value${parameter.type.charAt(0).toUpperCase()}${parameter.type.slice(1)}`;
  switch (parameter.type) {
    case "integer":
    case "positiveInt":
    case "unsignedInt": {
      const n = Number(raw);
      return Number.isInteger(n) ? { [field]: n } : undefined;
    }
    case "decimal": {
      const n = Number(raw);
      return Number.isFinite(n) ? { [field]: n } : undefined;
    }
    case "boolean":
      return raw === "true" || raw === "false" ? { [field]: raw === "true" } : undefined;
    default:
      // integer64 travels as a JSON string in FHIR R5+.
      return { [field]: raw };
  }
}

/** The `parameters` Parameters of a `$sql-run`, or none when nothing is bound. */
export function runParameters(query: SqlQuery): Record<string, unknown> | undefined {
  const parameter = query.parameters.flatMap((p) => {
    const value = typedValue(p);
    return value ? [{ name: p.name.trim(), ...value }] : [];
  });
  return parameter.length > 0 ? { resourceType: "Parameters", parameter } : undefined;
}

/** Whether a typed value is malformed for its type (shown inline). */
export function invalidValue(parameter: SqlParameter): boolean {
  return !!parameter.value?.trim() && typedValue(parameter) === undefined;
}

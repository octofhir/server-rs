import { fhirClient } from "@/shared/api/fhirClient";

// Server-backed console environments + {{variable}} resolution (octofhir-console IG).

const ANON = "anonymous";
let currentUserId = ANON;

export function setEnvironmentUser(userId: string | null | undefined): void {
  currentUserId = userId || ANON;
}

export interface EnvVariable {
  key: string;
  value: string;
  secret?: boolean;
}

export interface Environment {
  id: string;
  name: string;
  variables: EnvVariable[];
}

interface EnvVariableResource {
  key: string;
  initialValue?: string;
  currentValue?: string;
  secret?: boolean;
}

interface EnvironmentResource {
  [key: string]: unknown;
  resourceType: "ConsoleEnvironment";
  id?: string;
  name: string;
  owner: string;
  scope?: string;
  variable?: EnvVariableResource[];
}

function fromResource(r: EnvironmentResource): Environment {
  return {
    id: r.id ?? "",
    name: r.name,
    variables: (r.variable ?? []).map((v) => ({
      key: v.key,
      value: v.currentValue ?? v.initialValue ?? "",
      secret: v.secret,
    })),
  };
}

function toVariableResources(vars: EnvVariable[]): EnvVariableResource[] {
  return vars
    .filter((v) => v.key.trim().length > 0)
    .map((v) => ({
      key: v.key,
      initialValue: v.value || undefined,
      currentValue: v.value || undefined,
      secret: v.secret || undefined,
    }));
}

// --- active environment id (single shared store across hook instances) ---

const ACTIVE_KEY = "octofhir.console.activeEnv";
let activeEnvId: string | null = (() => {
  try {
    return localStorage.getItem(ACTIVE_KEY);
  } catch {
    return null;
  }
})();
const activeEnvListeners = new Set<() => void>();

export function getActiveEnvId(): string | null {
  return activeEnvId;
}

export function setActiveEnvId(id: string | null): void {
  activeEnvId = id;
  try {
    if (id) localStorage.setItem(ACTIVE_KEY, id);
    else localStorage.removeItem(ACTIVE_KEY);
  } catch {
    /* ignore */
  }
  for (const l of activeEnvListeners) l();
}

export function subscribeActiveEnv(cb: () => void): () => void {
  activeEnvListeners.add(cb);
  return () => activeEnvListeners.delete(cb);
}

// --- active variables, resolved into {{var}} at send time ---

let activeVariables: Record<string, string> = {};

export function setActiveVariables(vars: Record<string, string>): void {
  activeVariables = vars;
}

export function getActiveVariableNames(): string[] {
  return Object.keys(activeVariables);
}

function dynamicVar(name: string): string | null {
  switch (name) {
    case "$guid":
    case "$uuid":
      return crypto.randomUUID();
    case "$now":
    case "$isoTimestamp":
      return new Date().toISOString();
    case "$timestamp":
      return String(Date.now());
    case "$randomInt":
      return String(Math.floor(Math.random() * 1_000_000));
    default:
      return null;
  }
}

/** Replace {{var}} / {{$dynamic}} placeholders using the active environment. */
export function resolveVariables(input: string): string {
  if (!input || !input.includes("{{")) return input;
  return input.replace(/\{\{\s*([^}]+?)\s*\}\}/g, (match, raw: string) => {
    const name = raw.trim();
    if (name.startsWith("$")) {
      const dyn = dynamicVar(name);
      return dyn ?? match;
    }
    return name in activeVariables ? activeVariables[name] : match;
  });
}

/** Resolve every value of a headers map. */
export function resolveHeaders(headers: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(headers)) out[resolveVariables(k)] = resolveVariables(v);
  return out;
}

export const environmentService = {
  async list(): Promise<Environment[]> {
    if (currentUserId === ANON) return [];
    const bundle = await fhirClient.search<EnvironmentResource>("ConsoleEnvironment", {
      owner: currentUserId,
      _count: 100,
    });
    return (bundle.entry ?? [])
      .map((e) => e.resource)
      .filter((r): r is EnvironmentResource => !!r)
      .map(fromResource);
  },

  async create(name: string): Promise<Environment> {
    // Omit `variable` entirely — FHIR rejects an empty array.
    const created = (await fhirClient.create({
      resourceType: "ConsoleEnvironment",
      name,
      owner: currentUserId,
      scope: "global",
    })) as unknown as EnvironmentResource;
    return { id: created.id ?? "", name, variables: [] };
  },

  async update(env: Environment): Promise<void> {
    const variable = toVariableResources(env.variables);
    await fhirClient.update({
      resourceType: "ConsoleEnvironment",
      id: env.id,
      name: env.name,
      owner: currentUserId,
      scope: "global",
      // Omit when empty — FHIR rejects an empty array.
      ...(variable.length > 0 ? { variable } : {}),
    } as never);
  },

  async remove(id: string): Promise<void> {
    await fhirClient.delete("ConsoleEnvironment", id);
  },
};

import type { StoreWritable } from "effector";
import { type Adapter, persist as corePersist, type StorageAdapter } from "effector-storage";
import { fhirClient } from "./fhirClient";

/**
 * Generic per-user preference client backed by the `UserPreference` FHIR resource
 * (octofhir-ui IG). Preferences are keyed by `(user, namespace, key)` and stored
 * server-side so they follow the user across browsers.
 *
 * This module also exposes an effector-storage adapter (`persistPreference`) so an
 * effector store can transparently persist to a UserPreference instead of localStorage.
 *
 * Note: the active user is resolved at read/write time from `currentUserId`. Call
 * `setPreferencesUser` once the logged-in user is known (see usePreferenceUserSync),
 * before the consuming stores are first read. Switching users mid-session does not
 * retroactively rehydrate already-mounted stores until the next reload.
 */

const ANON = "anonymous";

interface UserPreferenceResource {
  [key: string]: unknown;
  resourceType: "UserPreference";
  id?: string;
  user: string;
  namespace: string;
  key: string;
  value: string;
  updatedAt?: string;
}

let currentUserId = ANON;

// Cache of loaded prefs: `${namespace}::${key}` -> { id, value }
const cache = new Map<string, { id?: string; value: string }>();
// In-flight / completed namespace loads, so we fetch each namespace once per user.
const namespaceLoads = new Map<string, Promise<void>>();

const cacheKey = (namespace: string, key: string) => `${namespace}::${key}`;

async function loadNamespace(namespace: string): Promise<void> {
  // Not signed in yet — skip server reads (avoids 401 before auth resolves).
  if (currentUserId === ANON) return;
  const existing = namespaceLoads.get(namespace);
  if (existing) return existing;

  const load = (async () => {
    try {
      const bundle = await fhirClient.search<UserPreferenceResource>("UserPreference", {
        user: currentUserId,
        namespace,
      });
      for (const entry of bundle.entry ?? []) {
        const r = entry.resource;
        if (!r) continue;
        cache.set(cacheKey(namespace, r.key), { id: r.id, value: r.value });
      }
    } catch {
      // Treat failures as "no stored prefs" — fall back to store defaults.
    }
  })();

  namespaceLoads.set(namespace, load);
  return load;
}

async function upsert(namespace: string, key: string, value: string): Promise<void> {
  // Not signed in yet — skip server writes (avoids 401 before auth resolves).
  if (currentUserId === ANON) return;
  await loadNamespace(namespace);
  const k = cacheKey(namespace, key);
  const existing = cache.get(k);
  const base: UserPreferenceResource = {
    resourceType: "UserPreference",
    user: currentUserId,
    namespace,
    key,
    value,
    updatedAt: new Date().toISOString(),
  };

  if (existing?.id) {
    await fhirClient.update({ ...base, id: existing.id });
    cache.set(k, { id: existing.id, value });
  } else {
    const created = (await fhirClient.create(base)) as unknown as UserPreferenceResource;
    cache.set(k, { id: created.id, value });
  }
}

/** Set the active user. Resets the cache so the next reads load that user's prefs. */
export function setPreferencesUser(userId: string | null | undefined): void {
  const next = userId || ANON;
  if (next === currentUserId) return;
  currentUserId = next;
  cache.clear();
  namespaceLoads.clear();
}

/** Read a single preference value (already-parsed JSON), or undefined if unset. */
export async function getPreference<T>(namespace: string, key: string): Promise<T | undefined> {
  await loadNamespace(namespace);
  const hit = cache.get(cacheKey(namespace, key));
  if (!hit) return undefined;
  try {
    return JSON.parse(hit.value) as T;
  } catch {
    return undefined;
  }
}

/** Write a single preference value (JSON-serialized). */
export async function setPreference<T>(namespace: string, key: string, value: T): Promise<void> {
  await upsert(namespace, key, JSON.stringify(value));
}

function preferenceAdapter(namespace: string): StorageAdapter {
  const adapter = (<State>(key: string): Adapter<State> => {
    return {
      async get(): Promise<State | undefined> {
        return getPreference<State>(namespace, key);
      },
      async set(value: State): Promise<void> {
        await upsert(namespace, key, JSON.stringify(value));
      },
    };
  }) as StorageAdapter;
  // Scope key collisions to the namespace.
  adapter.keyArea = namespace;
  return adapter;
}

/**
 * Persist an effector store to a `UserPreference` resource, the server-backed
 * equivalent of `effector-storage/local`'s `persist`.
 */
export function persistPreference<State>(opts: {
  store: StoreWritable<State>;
  key: string;
  namespace?: string;
}) {
  return corePersist({
    store: opts.store,
    key: opts.key,
    adapter: preferenceAdapter(opts.namespace ?? "console"),
  });
}

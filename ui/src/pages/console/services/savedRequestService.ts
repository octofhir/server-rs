import { fhirClient } from "@/shared/api/fhirClient";

// Server-backed saved requests & collections (octofhir-console IG).
// Stored at root `/ConsoleSavedRequest` and `/ConsoleCollection` (internal resources).

const ANON = "anonymous";
let currentUserId = ANON;

export function setSavedRequestUser(userId: string | null | undefined): void {
  currentUserId = userId || ANON;
}

interface SavedRequestResource {
  [key: string]: unknown;
  resourceType: "ConsoleSavedRequest";
  id?: string;
  name: string;
  owner: string;
  collection?: string;
  method: string;
  path: string;
  headers?: string;
  body?: string;
  order?: number;
  tag?: string[];
}

interface CollectionResource {
  [key: string]: unknown;
  resourceType: "ConsoleCollection";
  id?: string;
  name: string;
  owner: string;
  description?: string;
  visibility?: string;
}

export interface SavedRequest {
  id: string;
  name: string;
  collection?: string;
  method: string;
  path: string;
  headers?: Record<string, string>;
  body?: string;
}

export interface Collection {
  id: string;
  name: string;
  description?: string;
}

function nonEmpty(s: string | undefined): string | undefined {
  return s && s.trim().length > 0 ? s : undefined;
}

function toSaved(r: SavedRequestResource): SavedRequest {
  let headers: Record<string, string> | undefined;
  if (r.headers) {
    try {
      headers = JSON.parse(r.headers);
    } catch {
      headers = undefined;
    }
  }
  return {
    id: r.id ?? crypto.randomUUID(),
    name: r.name,
    collection: r.collection,
    method: r.method,
    path: r.path,
    headers,
    body: r.body,
  };
}

export const savedRequestService = {
  async listCollections(): Promise<Collection[]> {
    if (currentUserId === ANON) return [];
    const bundle = await fhirClient.search<CollectionResource>("ConsoleCollection", {
      owner: currentUserId,
      _count: 100,
    });
    return (bundle.entry ?? [])
      .map((e) => e.resource)
      .filter((r): r is CollectionResource => !!r)
      .map((r) => ({ id: r.id ?? "", name: r.name, description: r.description }));
  },

  async createCollection(name: string, description?: string): Promise<Collection> {
    const created = (await fhirClient.create({
      resourceType: "ConsoleCollection",
      name,
      owner: currentUserId,
      description: nonEmpty(description),
      visibility: "private",
    })) as unknown as CollectionResource;
    return { id: created.id ?? "", name, description };
  },

  async deleteCollection(id: string): Promise<void> {
    await fhirClient.delete("ConsoleCollection", id);
  },

  async listRequests(): Promise<SavedRequest[]> {
    if (currentUserId === ANON) return [];
    const bundle = await fhirClient.search<SavedRequestResource>("ConsoleSavedRequest", {
      owner: currentUserId,
      _count: 200,
    });
    return (bundle.entry ?? [])
      .map((e) => e.resource)
      .filter((r): r is SavedRequestResource => !!r)
      .map(toSaved);
  },

  async saveRequest(input: {
    name: string;
    collection?: string;
    method: string;
    path: string;
    headers?: Record<string, string>;
    body?: string;
  }): Promise<string> {
    const id = crypto.randomUUID();
    if (currentUserId === ANON) return id;
    const resource: SavedRequestResource = {
      resourceType: "ConsoleSavedRequest",
      id,
      name: input.name,
      owner: currentUserId,
      collection: input.collection,
      method: input.method,
      path: input.path,
      headers:
        input.headers && Object.keys(input.headers).length > 0
          ? JSON.stringify(input.headers)
          : undefined,
      body: nonEmpty(input.body),
    };
    const created = (await fhirClient.create(resource)) as unknown as SavedRequestResource;
    return created.id ?? id;
  },

  async deleteRequest(id: string): Promise<void> {
    await fhirClient.delete("ConsoleSavedRequest", id);
  },
};

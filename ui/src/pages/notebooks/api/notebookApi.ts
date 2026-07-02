// Notebook persistence — generic FHIR CRUD against /fhir/Notebook (the custom
// resource registered by the octofhir-notebooks IG). No bespoke endpoint needed.

import { fhirClient } from "@/shared/api/fhirClient";
import type { FhirResource } from "@/shared/api/types";
import type { Notebook } from "../model/notebook";

type NotebookResource = Notebook & FhirResource;

export interface NotebookListItem {
  id: string;
  title: string;
  description?: string;
  tags?: string[];
  cellCount: number;
  fhirVersion?: string;
  lastUpdated?: string;
}

export async function listNotebooks(): Promise<NotebookListItem[]> {
  const bundle = await fhirClient.search<NotebookResource>("Notebook", { _count: 100 });
  return (bundle.entry ?? [])
    .map((e) => e.resource)
    .filter((r): r is NotebookResource => !!r)
    .map((nb) => ({
      id: nb.id ?? "",
      title: nb.title ?? "Untitled",
      description: nb.description,
      tags: nb.tags,
      cellCount: nb.cells?.length ?? 0,
      fhirVersion: nb.fhirVersion,
      lastUpdated: nb.meta?.lastUpdated,
    }));
}

export async function readNotebook(id: string): Promise<Notebook> {
  return (await fhirClient.read<NotebookResource>("Notebook", id)) as Notebook;
}

// Cells are polymorphic/open content; the custom Notebook StructureDefinition only
// models header fields, so strict server validation would reject cell bodies. The
// frontend (and later the octofhir-notebook crate) owns the .fhirnb schema, so we
// skip server-side structural validation on writes via X-Skip-Validation.
const SKIP_VALIDATION = { "X-Skip-Validation": "true" };

export async function createNotebook(nb: Notebook): Promise<Notebook> {
  const { id: _omit, ...payload } = nb;
  const res = await fhirClient.customRequest({
    method: "POST",
    url: "/fhir/Notebook",
    data: payload,
    headers: SKIP_VALIDATION,
  });
  return res.data as Notebook;
}

export async function saveNotebook(nb: Notebook): Promise<Notebook> {
  if (!nb.id) return createNotebook(nb);
  const res = await fhirClient.customRequest({
    method: "PUT",
    url: `/fhir/Notebook/${nb.id}`,
    data: nb,
    headers: SKIP_VALIDATION,
  });
  return res.data as Notebook;
}

export async function deleteNotebook(id: string): Promise<void> {
  await fhirClient.delete("Notebook", id);
}

export type ExportFormat = "fhirnb" | "ipynb" | "bundle" | "markdown" | "html";
export type ImportFormat = "fhirnb" | "ipynb" | "bundle";

/** Download a saved notebook in the given format via the server converter. */
export async function exportNotebook(id: string, format: ExportFormat): Promise<void> {
  const res = await fetch(`/api/notebooks/${id}/export?format=${format}`, {
    credentials: "include",
  });
  if (!res.ok) throw new Error(`Export failed: HTTP ${res.status}`);
  const blob = await res.blob();
  const cd = res.headers.get("content-disposition") ?? "";
  const filename = /filename="([^"]+)"/.exec(cd)?.[1] ?? `notebook.${format}`;
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

/** Execute the whole notebook server-side (headless) and return it with outputs. */
export async function runNotebookHeadless(id: string): Promise<Notebook> {
  const res = await fetch(`/api/notebooks/${id}/run`, {
    method: "POST",
    credentials: "include",
  });
  if (!res.ok) throw new Error(`Run failed: HTTP ${res.status}`);
  return (await res.json()) as Notebook;
}

/** Convert an uploaded document (parsed JSON) into a Notebook (server-side). */
export async function importNotebook(doc: unknown, format: ImportFormat): Promise<Notebook> {
  const res = await fetch(`/api/notebooks/import?format=${format}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    body: JSON.stringify(doc),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Import failed: HTTP ${res.status} ${text}`);
  }
  return (await res.json()) as Notebook;
}

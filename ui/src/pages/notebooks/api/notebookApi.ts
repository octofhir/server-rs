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

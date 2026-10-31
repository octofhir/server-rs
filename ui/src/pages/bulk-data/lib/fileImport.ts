import { fhirClient } from "@/shared/api/fhirClient";
import type { FhirResource } from "@/shared/api/types";

/** Resources per batch Bundle. */
export const BATCH_SIZE = 200;

export interface ImportProgress {
  /** Resources read from the file so far. */
  read: number;
  written: number;
  failed: number;
  /** First few failures, `Type/id: message`. */
  errors: string[];
}

const MAX_ERRORS = 20;

/** Lines of a text file, read as a stream so large files never sit in memory whole. */
export async function* lines(file: Blob): AsyncGenerator<string> {
  const reader = file.stream().pipeThrough(new TextDecoderStream()).getReader();
  let rest = "";
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    rest += value;
    const parts = rest.split("\n");
    rest = parts.pop() ?? "";
    yield* parts;
  }
  if (rest) yield rest;
}

/**
 * Batch entry for one resource: `PUT Type/id` keeps the id the file carries (as `$import`
 * does), resources without an id are created.
 */
export function batchEntry(resource: FhirResource) {
  return resource.id
    ? { resource, request: { method: "PUT", url: `${resource.resourceType}/${resource.id}` } }
    : { resource, request: { method: "POST", url: resource.resourceType } };
}

interface BatchResponseEntry {
  response?: { status?: string; outcome?: { issue?: { diagnostics?: string }[] } };
}

async function send(resources: FhirResource[], progress: ImportProgress): Promise<void> {
  const bundle = { resourceType: "Bundle", type: "batch", entry: resources.map(batchEntry) };
  try {
    const response = await fhirClient.rawRequest("POST", "/fhir", bundle, {
      headers: { "Content-Type": "application/fhir+json", Accept: "application/fhir+json" },
    });
    const entries = ((response.data as { entry?: BatchResponseEntry[] } | undefined)?.entry ?? []) as BatchResponseEntry[];
    resources.forEach((resource, index) => {
      const status = entries[index]?.response?.status ?? "";
      if (/^2\d\d/.test(status)) {
        progress.written += 1;
        return;
      }
      progress.failed += 1;
      if (progress.errors.length < MAX_ERRORS) {
        const message = entries[index]?.response?.outcome?.issue?.[0]?.diagnostics ?? (status || "no response");
        progress.errors.push(`${resource.resourceType}/${resource.id ?? "(new)"}: ${message}`);
      }
    });
  } catch (error) {
    progress.failed += resources.length;
    if (progress.errors.length < MAX_ERRORS) {
      progress.errors.push(`Batch of ${resources.length}: ${error instanceof Error ? error.message : "request failed"}`);
    }
  }
}

/**
 * Imports an NDJSON file (or a FHIR Bundle / single resource in JSON) through batch Bundles.
 * Lines that aren't JSON resources count as failures; `onProgress` gets a fresh copy after
 * each batch.
 */
export async function importFile(
  file: File,
  signal: AbortSignal,
  onProgress: (progress: ImportProgress) => void
): Promise<ImportProgress> {
  const progress: ImportProgress = { read: 0, written: 0, failed: 0, errors: [] };
  const report = () => onProgress({ ...progress, errors: [...progress.errors] });
  let batch: FhirResource[] = [];

  const flush = async () => {
    if (batch.length === 0) return;
    const current = batch;
    batch = [];
    await send(current, progress);
    report();
  };

  const accept = async (value: unknown, where: string) => {
    if (value && typeof value === "object" && typeof (value as FhirResource).resourceType === "string") {
      batch.push(value as FhirResource);
      progress.read += 1;
      if (batch.length >= BATCH_SIZE) await flush();
      return;
    }
    progress.failed += 1;
    if (progress.errors.length < MAX_ERRORS) progress.errors.push(`${where}: not a FHIR resource`);
  };

  // A .json file holds one document: a Bundle (its entries are imported) or a single resource.
  if (file.name.toLowerCase().endsWith(".json")) {
    let document: unknown;
    try {
      document = JSON.parse(await file.text());
    } catch {
      return { ...progress, failed: 1, errors: ["The file is not valid JSON"] };
    }
    const entries = (document as { resourceType?: string; entry?: { resource?: unknown }[] }).resourceType === "Bundle"
      ? ((document as { entry?: { resource?: unknown }[] }).entry ?? []).map((entry) => entry.resource)
      : [document];
    for (const [index, resource] of entries.entries()) {
      if (signal.aborted) break;
      await accept(resource, `Entry ${index + 1}`);
    }
  } else {
    let lineNumber = 0;
    for await (const line of lines(file)) {
      if (signal.aborted) break;
      lineNumber += 1;
      if (!line.trim()) continue;
      let value: unknown;
      try {
        value = JSON.parse(line);
      } catch {
        value = undefined;
      }
      await accept(value, `Line ${lineNumber}`);
    }
  }
  if (!signal.aborted) await flush();
  report();
  return progress;
}

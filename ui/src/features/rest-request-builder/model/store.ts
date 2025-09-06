import { createEffect, createEvent, createStore } from "effector";
import type { HttpMethod, HttpRequestConfig, HttpResponse } from "@/shared/api/types";
import { fhirClient } from "@/shared/api/fhirClient";

export type HeadersMap = Record<string, string>;

export interface RestRequestState {
  method: HttpMethod;
  path: string; // relative path e.g. "/Patient/123"
  headers: HeadersMap;
  body: string; // raw JSON string
}

export interface SendPayload {
  request: RestRequestState;
  baseUrl: string;
  timeout: number;
}

export interface SendResult<T = any> {
  response: HttpResponse<T>;
  durationMs: number;
  sizeBytes: number | null;
}

// Events to update form state
export const setMethod = createEvent<HttpMethod>();
export const setPath = createEvent<string>();
export const setHeader = createEvent<{ key: string; value: string }>();
export const removeHeader = createEvent<string>();
export const setBody = createEvent<string>();
export const resetRequest = createEvent();

// Main store
export const $restRequest = createStore<RestRequestState>({
  method: "GET",
  path: "/metadata",
  headers: {},
  body: "",
});

$restRequest
  .on(setMethod, (state, method) => ({ ...state, method }))
  .on(setPath, (state, path) => ({ ...state, path }))
  .on(setHeader, (state, { key, value }) => ({
    ...state,
    headers: { ...state.headers, [key]: value },
  }))
  .on(removeHeader, (state, key) => {
    const { [key]: _, ...rest } = state.headers;
    return { ...state, headers: rest };
  })
  .on(setBody, (state, body) => ({ ...state, body }))
  .reset(resetRequest);

// Effect to send request
export const sendRequestFx = createEffect(async (payload: SendPayload): Promise<SendResult> => {
  const { request, baseUrl, timeout } = payload;

  // Configure client
  fhirClient.setBaseUrl(baseUrl);
  fhirClient.setTimeout(timeout);

  let data: any = undefined;
  if (request.body && request.body.trim().length > 0) {
    try {
      data = JSON.parse(request.body);
    } catch (e) {
      throw new Error("Invalid JSON in request body");
    }
  }

  const config: HttpRequestConfig = {
    method: request.method,
    url: request.path.startsWith("/") ? request.path : `/${request.path}`,
    headers: request.headers,
    data,
    timeout,
  };

  const started = performance.now();
  const resp = await fhirClient.customRequest(config);
  const durationMs = Math.round(performance.now() - started);

  // Estimate size from header or compute from data
  const contentLength = resp.headers["content-length"];
  let sizeBytes: number | null = null;
  if (contentLength) {
    const parsed = Number(contentLength);
    sizeBytes = Number.isFinite(parsed) ? parsed : null;
  } else {
    try {
      const text = typeof resp.data === "string" ? resp.data : JSON.stringify(resp.data);
      sizeBytes = new Blob([text]).size;
    } catch {
      sizeBytes = null;
    }
  }

  return { response: resp, durationMs, sizeBytes };
});

export const setCommonHeader = createEvent<"Accept" | "Content-Type">();
$restRequest.on(setCommonHeader, (state, header) => {
  const map: Record<string, string> = {
    Accept: "application/fhir+json",
    "Content-Type": "application/fhir+json",
  };
  return { ...state, headers: { ...state.headers, [header]: map[header] } };
});

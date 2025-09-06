import { createEvent, createStore } from "effector";
import type { HttpResponse } from "@/shared/api/types";

export interface ResponseState {
  loading: boolean;
  response: HttpResponse | null;
  durationMs: number | null;
  sizeBytes: number | null;
  error: string | null;
}

export const setResponseState = createEvent<Partial<ResponseState>>();
export const setResponseError = createEvent<string | null>();
export const resetResponse = createEvent();

export const $responseState = createStore<ResponseState>({
  loading: false,
  response: null,
  durationMs: null,
  sizeBytes: null,
  error: null,
});

$responseState
  .on(setResponseState, (state, patch) => ({ ...state, ...patch }))
  .on(setResponseError, (state, error) => ({ ...state, error }))
  .reset(resetResponse);

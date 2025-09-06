import { createEvent, createStore } from "effector";
import type { HttpMethod } from "@/shared/api/types";
import { storage } from "@/shared/lib/storage";

export interface RestHistoryItem {
  id: string;
  timestamp: string;
  method: HttpMethod;
  path: string;
  status: number;
  duration: number;
  success: boolean;
}

const STORAGE_KEY = "restConsole:history";

export const addHistoryItem = createEvent<RestHistoryItem>();
export const clearHistory = createEvent();

export const $history = createStore<RestHistoryItem[]>(storage.getItem(STORAGE_KEY, []) ?? [])
  .on(addHistoryItem, (list, item) => {
    const next = [item, ...list].slice(0, 20);
    storage.setItem(STORAGE_KEY, next);
    return next;
  })
  .on(clearHistory, () => {
    storage.removeItem(STORAGE_KEY);
    return [];
  });

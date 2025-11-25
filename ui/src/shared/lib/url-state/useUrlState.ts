import { createSignal, createEffect, onCleanup, type Accessor, type Setter } from "solid-js";
import type { UrlStateConfig } from "./urlStateManager";
import { createUrlStateManager } from "./urlStateManager";

/**
 * SolidJS hook for syncing state with URL query parameters
 * @param key - The URL parameter key
 * @param config - Configuration for serialization, deserialization, and default value
 * @returns Tuple of [value accessor, setter function]
 */
export function useUrlState<T>(
  key: string,
  config: UrlStateConfig<T>
): [Accessor<T>, (value: T | ((prev: T) => T)) => void] {
  const manager = createUrlStateManager(key, config);
  const [value, setValue] = createSignal<T>(manager.get());

  // Subscribe to URL changes (popstate events)
  createEffect(() => {
    const unsubscribe = manager.subscribe((newValue) => {
      setValue(() => newValue);
    });

    onCleanup(unsubscribe);
  });

  const setUrlState = (newValue: T | ((prev: T) => T)) => {
    const resolvedValue =
      typeof newValue === "function" ? (newValue as (prev: T) => T)(value()) : newValue;

    manager.set(resolvedValue);
    setValue(() => resolvedValue);
  };

  return [value, setUrlState];
}

import { useCallback, useEffect, useState } from "react";
import type { UrlStateConfig } from "./urlStateManager";
import { createUrlStateManager } from "./urlStateManager";

export function useUrlState<T>(
  key: string,
  config: UrlStateConfig<T>
): [T, (value: T | ((prev: T) => T)) => void] {
  const manager = createUrlStateManager(key, config);
  const [value, setValue] = useState<T>(manager.get);

  useEffect(() => {
    // Sync initial value from URL
    setValue(manager.get());

    // Subscribe to URL changes
    const unsubscribe = manager.subscribe((newValue) => {
      setValue(newValue);
    });

    return unsubscribe;
  }, [manager]);

  const setUrlState = useCallback(
    (newValue: T | ((prev: T) => T)) => {
      const resolvedValue =
        typeof newValue === "function" ? (newValue as (prev: T) => T)(value) : newValue;

      manager.set(resolvedValue);
      setValue(resolvedValue);
    },
    [manager, value]
  );

  return [value, setUrlState];
}

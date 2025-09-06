export interface UrlStateConfig<T> {
  serialize: (value: T) => string;
  deserialize: (value: string) => T;
  defaultValue: T;
}

export interface UrlStateManager<T> {
  get: () => T;
  set: (value: T) => void;
  subscribe: (listener: (value: T) => void) => () => void;
}

export function createUrlStateManager<T>(
  key: string,
  config: UrlStateConfig<T>
): UrlStateManager<T> {
  const listeners = new Set<(value: T) => void>();

  const getCurrentValue = (): T => {
    if (typeof window === "undefined") {
      return config.defaultValue;
    }

    const urlParams = new URLSearchParams(window.location.search);
    const rawValue = urlParams.get(key);

    if (rawValue === null) {
      return config.defaultValue;
    }

    try {
      return config.deserialize(rawValue);
    } catch (error) {
      console.warn(`Failed to deserialize URL param "${key}":`, error);
      return config.defaultValue;
    }
  };

  const updateUrl = (value: T) => {
    if (typeof window === "undefined") {
      return;
    }

    const url = new URL(window.location.href);
    const serialized = config.serialize(value);

    if (serialized === config.serialize(config.defaultValue)) {
      // Remove param if it's the default value
      url.searchParams.delete(key);
    } else {
      url.searchParams.set(key, serialized);
    }

    // Use replaceState to avoid creating browser history entries for every change
    window.history.replaceState(null, "", url.toString());
  };

  const setValue = (value: T) => {
    updateUrl(value);
    listeners.forEach((listener) => listener(value));
  };

  const subscribe = (listener: (value: T) => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  };

  // Listen to popstate events to handle browser back/forward
  if (typeof window !== "undefined") {
    const handlePopState = () => {
      const value = getCurrentValue();
      listeners.forEach((listener) => listener(value));
    };

    window.addEventListener("popstate", handlePopState);
  }

  return {
    get: getCurrentValue,
    set: setValue,
    subscribe,
  };
}

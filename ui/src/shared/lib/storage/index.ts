export interface StorageAdapter {
  getItem<T>(key: string, defaultValue?: T): T | null;
  setItem<T>(key: string, value: T): void;
  removeItem(key: string): void;
  clear(): void;
}

export class LocalStorageAdapter implements StorageAdapter {
  private prefix: string;

  constructor(prefix = "octofhir:") {
    this.prefix = prefix;
  }

  private getFullKey(key: string): string {
    return `${this.prefix}${key}`;
  }

  getItem<T>(key: string, defaultValue?: T): T | null {
    try {
      const fullKey = this.getFullKey(key);
      const item = localStorage.getItem(fullKey);

      if (item === null) {
        return defaultValue ?? null;
      }

      return JSON.parse(item) as T;
    } catch (error) {
      console.warn(`Failed to get item from localStorage:`, error);
      return defaultValue ?? null;
    }
  }

  setItem<T>(key: string, value: T): void {
    try {
      const fullKey = this.getFullKey(key);
      localStorage.setItem(fullKey, JSON.stringify(value));
    } catch (error) {
      console.warn(`Failed to set item in localStorage:`, error);
    }
  }

  removeItem(key: string): void {
    try {
      const fullKey = this.getFullKey(key);
      localStorage.removeItem(fullKey);
    } catch (error) {
      console.warn(`Failed to remove item from localStorage:`, error);
    }
  }

  clear(): void {
    try {
      const keys = Object.keys(localStorage);
      for (const key of keys) {
        if (key.startsWith(this.prefix)) {
          localStorage.removeItem(key);
        }
      }
    } catch (error) {
      console.warn(`Failed to clear localStorage:`, error);
    }
  }
}

// Default instance
export const storage = new LocalStorageAdapter();

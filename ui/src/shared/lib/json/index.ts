export interface JsonFormatterOptions {
  indent?: number;
  maxLength?: number;
  sort?: boolean;
}

export const formatJson = (value: unknown, options: JsonFormatterOptions = {}): string => {
  const { indent = 2, sort = false } = options;

  try {
    if (sort && typeof value === "object" && value !== null) {
      // Simple sort for object keys (recursive)
      const sortKeys = (obj: any): any => {
        if (Array.isArray(obj)) {
          return obj.map(sortKeys);
        }
        if (obj !== null && typeof obj === "object") {
          return Object.keys(obj)
            .sort()
            .reduce((result, key) => {
              result[key] = sortKeys(obj[key]);
              return result;
            }, {} as any);
        }
        return obj;
      };
      value = sortKeys(value);
    }

    return JSON.stringify(value, null, indent);
  } catch (_error) {
    return String(value);
  }
};

export const parseJson = (json: string): { data: unknown; error: string | null } => {
  try {
    const data = JSON.parse(json);
    return { data, error: null };
  } catch (error) {
    return {
      data: null,
      error: error instanceof Error ? error.message : "Invalid JSON",
    };
  }
};

export const isValidJson = (json: string): boolean => {
  try {
    JSON.parse(json);
    return true;
  } catch {
    return false;
  }
};

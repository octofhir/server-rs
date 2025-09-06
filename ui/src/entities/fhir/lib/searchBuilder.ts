export interface FhirSearchParams {
  [key: string]: string | number | boolean | string[] | undefined;
}

/**
 * Build FHIR search parameters with validation
 */
export class FhirSearchBuilder {
  private params: Map<string, string> = new Map();

  constructor(initialParams?: FhirSearchParams) {
    if (initialParams) {
      Object.entries(initialParams).forEach(([key, value]) => {
        this.setParam(key, value);
      });
    }
  }

  /**
   * Set a search parameter
   */
  setParam(key: string, value: string | number | boolean | string[] | undefined): this {
    if (value === undefined || value === null) {
      return this;
    }

    if (Array.isArray(value)) {
      this.params.set(key, value.join(","));
    } else {
      this.params.set(key, String(value));
    }

    return this;
  }

  /**
   * Set count parameter
   */
  count(count: number): this {
    return this.setParam("_count", Math.max(1, Math.min(1000, count)));
  }

  /**
   * Set offset parameter
   */
  offset(offset: number): this {
    return this.setParam("_offset", Math.max(0, offset));
  }

  /**
   * Set sort parameter
   */
  sort(field: string, order: "asc" | "desc" = "asc"): this {
    const sortValue = order === "desc" ? `-${field}` : field;
    return this.setParam("_sort", sortValue);
  }

  /**
   * Add include parameter
   */
  include(include: string): this {
    const existing = this.params.get("_include");
    if (existing) {
      this.params.set("_include", `${existing},${include}`);
    } else {
      this.params.set("_include", include);
    }
    return this;
  }

  /**
   * Add revinclude parameter
   */
  revinclude(revinclude: string): this {
    const existing = this.params.get("_revinclude");
    if (existing) {
      this.params.set("_revinclude", `${existing},${revinclude}`);
    } else {
      this.params.set("_revinclude", revinclude);
    }
    return this;
  }

  /**
   * Set elements parameter (return only specified fields)
   */
  elements(elements: string[]): this {
    return this.setParam("_elements", elements);
  }

  /**
   * Set summary parameter
   */
  summary(summary: "true" | "false" | "text" | "data" | "count"): this {
    return this.setParam("_summary", summary);
  }

  /**
   * Add date range filter
   */
  dateRange(field: string, from?: Date | string, to?: Date | string): this {
    if (from && to) {
      return this.setParam(field, `ge${formatDate(from)}&${field}=le${formatDate(to)}`);
    } else if (from) {
      return this.setParam(field, `ge${formatDate(from)}`);
    } else if (to) {
      return this.setParam(field, `le${formatDate(to)}`);
    }
    return this;
  }

  /**
   * Add text search
   */
  text(query: string): this {
    return this.setParam("_text", query);
  }

  /**
   * Add content search
   */
  content(query: string): this {
    return this.setParam("_content", query);
  }

  /**
   * Add identifier search
   */
  identifier(system: string, value: string): this {
    return this.setParam("identifier", `${system}|${value}`);
  }

  /**
   * Add reference search
   */
  reference(field: string, resourceType: string, id: string): this {
    return this.setParam(field, `${resourceType}/${id}`);
  }

  /**
   * Add token search (for coded values)
   */
  token(field: string, system?: string, code?: string): this {
    if (system && code) {
      return this.setParam(field, `${system}|${code}`);
    } else if (code) {
      return this.setParam(field, code);
    }
    return this;
  }

  /**
   * Remove a parameter
   */
  remove(key: string): this {
    this.params.delete(key);
    return this;
  }

  /**
   * Clear all parameters
   */
  clear(): this {
    this.params.clear();
    return this;
  }

  /**
   * Build the final parameters object
   */
  build(): Record<string, string> {
    const result: Record<string, string> = {};
    this.params.forEach((value, key) => {
      result[key] = value;
    });
    return result;
  }

  /**
   * Build URL search string
   */
  toSearchString(): string {
    const params = new URLSearchParams();
    this.params.forEach((value, key) => {
      params.set(key, value);
    });
    return params.toString();
  }

  /**
   * Clone the builder
   */
  clone(): FhirSearchBuilder {
    const clone = new FhirSearchBuilder();
    this.params.forEach((value, key) => {
      clone.params.set(key, value);
    });
    return clone;
  }
}

/**
 * Format date for FHIR search
 */
function formatDate(date: Date | string): string {
  if (typeof date === "string") {
    return date;
  }
  return date.toISOString().split("T")[0];
}

/**
 * Create a new FHIR search builder
 */
export const createSearchBuilder = (initialParams?: FhirSearchParams): FhirSearchBuilder => {
  return new FhirSearchBuilder(initialParams);
};

/**
 * Common search parameter presets
 */
export const searchPresets = {
  active: () => createSearchBuilder().setParam("active", "true"),
  inactive: () => createSearchBuilder().setParam("active", "false"),
  recent: (days = 30) => {
    const date = new Date();
    date.setDate(date.getDate() - days);
    return createSearchBuilder().dateRange("_lastUpdated", date);
  },
  count: (count: number) => createSearchBuilder().count(count),
  summary: () => createSearchBuilder().summary("true"),
};

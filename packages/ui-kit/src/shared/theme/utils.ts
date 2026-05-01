/**
 * Convert camelCase to kebab-case.
 * "primaryHover" → "primary-hover", "primaryBg" → "primary-bg"
 */
function camelToKebab(str: string): string {
    return str.replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase();
}

/**
 * Generates an object of CSS variables from a theme object.
 * Recursively traverses the object and joins keys with hyphens.
 * camelCase keys are automatically converted to kebab-case.
 *
 * Example:
 * { accent: { primaryBg: '#...' } } -> { '--octo-accent-primary-bg': '#...' }
 */
export function generateCSSVariables(
    obj: Record<string, any>,
    prefix: string = "--octo",
    parentKey: string = ""
): Record<string, string> {
    const variables: Record<string, string> = {};

    for (const key in obj) {
        if (Object.prototype.hasOwnProperty.call(obj, key)) {
            const value = obj[key];
            const kebabKey = camelToKebab(key);
            const currentKey = parentKey ? `${parentKey}-${kebabKey}` : kebabKey;

            if (typeof value === "object" && value !== null && !Array.isArray(value)) {
                Object.assign(variables, generateCSSVariables(value, prefix, currentKey));
            } else {
                variables[`${prefix}-${currentKey}`] = String(value);
            }
        }
    }

    return variables;
}

type WidenLiteral<T> =
    T extends string ? string :
    T extends number ? number :
    T extends boolean ? boolean :
    T;

export type DeepWiden<T> =
    T extends (...args: any[]) => any ? T :
    T extends readonly (infer U)[] ? readonly DeepWiden<U>[] :
    T extends object ? { [K in keyof T]: DeepWiden<T[K]> } :
    WidenLiteral<T>;

export type DeepPartial<T> =
    T extends (...args: any[]) => any ? T :
    T extends readonly any[] ? T :
    T extends object ? { [K in keyof T]?: DeepPartial<WidenLiteral<T[K]>> } :
    WidenLiteral<T>;

export function mergeDeep<T extends Record<string, any>>(
    base: T,
    override: DeepPartial<T> | undefined,
): T {
    if (!override) return base;

    const result: Record<string, any> = { ...base };
    const overrideRecord = override as Record<string, any>;

    for (const key of Object.keys(overrideRecord)) {
        const overrideValue = overrideRecord[key];
        if (overrideValue === undefined) continue;

        const baseValue = base[key];
        if (
            isPlainObject(baseValue) &&
            isPlainObject(overrideValue)
        ) {
            result[key] = mergeDeep(baseValue, overrideValue as DeepPartial<typeof baseValue>);
        } else {
            result[key] = overrideValue;
        }
    }

    return result as T;
}

function isPlainObject(value: unknown): value is Record<string, any> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

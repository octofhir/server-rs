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

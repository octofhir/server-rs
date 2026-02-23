/**
 * Generates an object of CSS variables from a theme object.
 * Recursively traverses the object and joins keys with hyphens.
 * 
 * Example:
 * { colors: { primary: { 500: '#...' } } } -> { '--octo-colors-primary-500': '#...' }
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
            const currentKey = parentKey ? `${parentKey}-${key}` : key;

            if (typeof value === "object" && value !== null && !Array.isArray(value)) {
                Object.assign(variables, generateCSSVariables(value, prefix, currentKey));
            } else {
                variables[`${prefix}-${currentKey}`] = String(value);
            }
        }
    }

    return variables;
}

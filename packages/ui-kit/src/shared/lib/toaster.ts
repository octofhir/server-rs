import { Toaster, type ToastProps } from "@gravity-ui/uikit";

export const toaster = new Toaster();

export type NotifyTheme = NonNullable<ToastProps["theme"]>;

export interface NotifyOptions extends Omit<ToastProps, "name" | "theme"> {
    /** Optional unique id. Auto-generated if omitted. */
    name?: string;
    /** Visual theme. Default `info`. */
    theme?: NotifyTheme;
}

let counter = 0;
const nextId = () => `octo-toast-${++counter}-${Date.now().toString(36)}`;

/**
 * Shows a toast notification using Gravity UI's Toaster.
 *
 * @example
 *   notify({ theme: "success", title: "Saved", content: "Changes persisted" });
 *   notify({ theme: "danger", title: "Error", content: err.message });
 */
export function notify(options: NotifyOptions): string {
    const name = options.name ?? nextId();
    toaster.add({
        autoHiding: 5000,
        ...options,
        name,
        theme: options.theme ?? "info",
    });
    return name;
}

notify.remove = (name: string) => toaster.remove(name);
notify.clear = () => toaster.removeAll();

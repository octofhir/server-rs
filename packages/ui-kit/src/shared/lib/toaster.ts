import type { ReactNode } from "react";

export type NotifyTheme = "info" | "success" | "danger" | "warning";

export interface NotifyOptions {
    /** Optional unique id. Auto-generated if omitted. */
    name?: string;
    title?: ReactNode;
    /** Body content. */
    content?: ReactNode;
    /** Visual theme. Default `info`. */
    theme?: NotifyTheme;
    /** Auto-dismiss delay in ms, or `false` to keep until dismissed. Default 5000. */
    autoHiding?: number | false;
}

export interface Toast extends NotifyOptions {
    name: string;
    theme: NotifyTheme;
}

type Listener = (toasts: Toast[]) => void;

class ToastStore {
    private toasts: Toast[] = [];
    private listeners = new Set<Listener>();

    add(toast: Toast) {
        this.toasts = [...this.toasts.filter((t) => t.name !== toast.name), toast];
        this.emit();
    }

    remove(name: string) {
        this.toasts = this.toasts.filter((t) => t.name !== name);
        this.emit();
    }

    removeAll() {
        this.toasts = [];
        this.emit();
    }

    subscribe(listener: Listener) {
        this.listeners.add(listener);
        listener(this.toasts);
        return () => {
            this.listeners.delete(listener);
        };
    }

    private emit() {
        for (const l of this.listeners) l(this.toasts);
    }
}

export const toastStore = new ToastStore();

let counter = 0;
const nextId = () => {
    counter += 1;
    return `octo-toast-${counter}`;
};

/**
 * Shows a toast notification.
 *
 * @example
 *   notify({ theme: "success", title: "Saved", content: "Changes persisted" });
 *   notify({ theme: "danger", title: "Error", content: err.message });
 */
export function notify(options: NotifyOptions): string {
    const name = options.name ?? nextId();
    toastStore.add({
        autoHiding: 5000,
        ...options,
        name,
        theme: options.theme ?? "info",
    });
    return name;
}

notify.remove = (name: string) => toastStore.remove(name);
notify.clear = () => toastStore.removeAll();

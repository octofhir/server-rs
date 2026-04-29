import { ConfirmDialog } from "@gravity-ui/components";
import type { ButtonProps } from "@gravity-ui/uikit";
import { useEffect, useState, type ReactNode } from "react";

export type ConfirmTheme = "info" | "danger" | "success" | "warning" | "default";

export interface ConfirmOptions {
    title?: string;
    /** Body content rendered above the action buttons. */
    message?: ReactNode;
    /** Text for the confirm button. Default "OK". */
    confirmText?: string;
    /** Text for the cancel button. Default "Cancel". */
    cancelText?: string;
    /** Visual theme of the confirm button. Default `info`. */
    theme?: ConfirmTheme;
    onConfirm?: () => void | Promise<void>;
    onCancel?: () => void;
}

interface InternalEntry extends ConfirmOptions {
    id: string;
}

type Listener = (entries: InternalEntry[]) => void;

const themeToView = (t?: ConfirmTheme): ButtonProps["view"] | undefined => {
    if (t === "danger") return "outlined-danger";
    if (t === "success") return "outlined-success";
    if (t === "info") return "outlined-info";
    if (t === "warning") return "outlined-warning";
    return undefined;
};

class ConfirmStore {
    private entries: InternalEntry[] = [];
    private listeners = new Set<Listener>();

    open(options: ConfirmOptions): string {
        const id = Math.random().toString(36).slice(2);
        this.entries = [...this.entries, { ...options, id }];
        this.emit();
        return id;
    }

    close(id: string) {
        this.entries = this.entries.filter((e) => e.id !== id);
        this.emit();
    }

    closeAll() {
        this.entries = [];
        this.emit();
    }

    subscribe(listener: Listener) {
        this.listeners.add(listener);
        listener(this.entries);
        return () => {
            this.listeners.delete(listener);
        };
    }

    private emit() {
        for (const l of this.listeners) l(this.entries);
    }
}

const store = new ConfirmStore();

/**
 * Mounted once by `UIProvider`. Renders any modals opened via the imperative `confirm()` API.
 */
export function ConfirmModalHost() {
    const [entries, setEntries] = useState<InternalEntry[]>([]);

    useEffect(() => store.subscribe(setEntries), []);

    return (
        <>
            {entries.map((e) => {
                const close = () => store.close(e.id);
                const view = themeToView(e.theme);
                const propsButtonApply: ButtonProps | undefined = view ? { view } : undefined;
                return (
                    <ConfirmDialog
                        key={e.id}
                        open
                        onClose={() => {
                            e.onCancel?.();
                            close();
                        }}
                        title={e.title}
                        message={e.message}
                        textButtonApply={e.confirmText ?? "OK"}
                        textButtonCancel={e.cancelText ?? "Cancel"}
                        onClickButtonApply={async () => {
                            await e.onConfirm?.();
                            close();
                        }}
                        onClickButtonCancel={() => {
                            e.onCancel?.();
                            close();
                        }}
                        propsButtonApply={propsButtonApply}
                    />
                );
            })}
        </>
    );
}

/**
 * Imperative confirm dialog. Returns the dialog id; pass to `confirm.close(id)` to close early.
 *
 * @example
 *   confirm({
 *     title: "Delete user?",
 *     message: "This action cannot be undone.",
 *     theme: "danger",
 *     confirmText: "Delete",
 *     onConfirm: () => deleteUser(id),
 *   });
 */
export function confirm(options: ConfirmOptions): string {
    return store.open(options);
}

confirm.close = (id: string) => store.close(id);
confirm.closeAll = () => store.closeAll();

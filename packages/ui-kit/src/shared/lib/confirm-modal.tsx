import { useEffect, useState, type ReactNode } from "react";
import { ConfirmDialog } from "@gravity-ui/components";
import type { ButtonProps } from "@gravity-ui/uikit";

export interface OpenConfirmModalOptions {
    title?: ReactNode;
    /** Free-form body content rendered above the action buttons. */
    children?: ReactNode;
    /** Mantine-flavoured alias for `children`. */
    message?: ReactNode;
    labels?: { confirm?: string; cancel?: string };
    confirmProps?: ButtonProps;
    cancelProps?: ButtonProps;
    onConfirm?: () => void;
    onCancel?: () => void;
    /** Maps Mantine-style `color: "red"` to Gravity Button view. */
    confirmColor?: "red" | "blue" | "green" | "default";
}

interface InternalEntry extends OpenConfirmModalOptions {
    id: string;
}

type Listener = (entries: InternalEntry[]) => void;

class ConfirmModalStore {
    private entries: InternalEntry[] = [];
    private listeners = new Set<Listener>();

    open(options: OpenConfirmModalOptions): string {
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
        return () => {
            this.listeners.delete(listener);
        };
    }

    private emit() {
        for (const l of this.listeners) l(this.entries);
    }
}

const store = new ConfirmModalStore();

const reactNodeToString = (n?: ReactNode): string | undefined => {
    if (n === null || n === undefined) return undefined;
    if (typeof n === "string") return n;
    if (typeof n === "number" || typeof n === "boolean") return String(n);
    return undefined;
};

const colorToView = (
    c?: OpenConfirmModalOptions["confirmColor"],
): ButtonProps["view"] | undefined => {
    if (c === "red") return "outlined-danger";
    if (c === "green") return "outlined-success";
    if (c === "blue") return "outlined-info";
    return undefined;
};

/**
 * Mounted once by `UIProvider`. Renders any modals that consumers open via
 * the imperative `modals.openConfirmModal()` / `modals.confirm()` API.
 */
export function ConfirmModalHost() {
    const [entries, setEntries] = useState<InternalEntry[]>([]);

    useEffect(() => store.subscribe(setEntries), []);

    return (
        <>
            {entries.map((e) => {
                const close = () => store.close(e.id);
                const propsButtonApply: ButtonProps = {
                    ...(e.confirmProps ?? {}),
                };
                const colorView = colorToView(e.confirmColor);
                if (colorView && !propsButtonApply.view) propsButtonApply.view = colorView;
                return (
                    <ConfirmDialog
                        key={e.id}
                        open
                        onClose={() => {
                            e.onCancel?.();
                            close();
                        }}
                        title={reactNodeToString(e.title)}
                        message={e.message ?? e.children}
                        textButtonApply={e.labels?.confirm ?? "OK"}
                        textButtonCancel={e.labels?.cancel ?? "Cancel"}
                        onClickButtonApply={() => {
                            e.onConfirm?.();
                            close();
                        }}
                        onClickButtonCancel={() => {
                            e.onCancel?.();
                            close();
                        }}
                        propsButtonApply={propsButtonApply}
                        propsButtonCancel={e.cancelProps}
                    />
                );
            })}
        </>
    );
}

/**
 * Imperative confirm-modal API.
 * Use {@link ConfirmModalHost} once at the root (already done by `UIProvider`)
 * and call `modals.openConfirmModal(...)` from anywhere in the React tree.
 */
export const modals = {
    openConfirmModal: (options: OpenConfirmModalOptions) => store.open(options),
    confirm: (options: OpenConfirmModalOptions) => store.open(options),
    closeAll: () => store.closeAll(),
    close: (id: string) => store.close(id),
};

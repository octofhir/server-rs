import { useEffect, useState, type ReactNode } from "react";
import { Button, type ButtonProps } from "../ui/Button";
import { Modal } from "../ui/Modal";

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

const themeToView = (t?: ConfirmTheme): ButtonProps["view"] => {
    if (t === "danger") return "action-danger";
    if (t === "success") return "action-success";
    if (t === "warning") return "action-warning";
    return "action";
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
            {entries.map((entry) => {
                const close = () => store.close(entry.id);
                const cancel = () => {
                    entry.onCancel?.();
                    close();
                };
                const apply = async () => {
                    await entry.onConfirm?.();
                    close();
                };
                return (
                    <Modal
                        key={entry.id}
                        open
                        onClose={cancel}
                        title={entry.title}
                        size="sm"
                        footer={
                            <>
                                <Button view="flat" onClick={cancel}>
                                    {entry.cancelText ?? "Cancel"}
                                </Button>
                                <Button view={themeToView(entry.theme)} onClick={apply}>
                                    {entry.confirmText ?? "OK"}
                                </Button>
                            </>
                        }
                    >
                        {entry.message}
                    </Modal>
                );
            })}
        </>
    );
}

/**
 * Imperative confirm dialog. Returns the dialog id; pass to `confirm.close(id)` to close early.
 */
export function confirm(options: ConfirmOptions): string {
    return store.open(options);
}

confirm.close = (id: string) => store.close(id);
confirm.closeAll = () => store.closeAll();

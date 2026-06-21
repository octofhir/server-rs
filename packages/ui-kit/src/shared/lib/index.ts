export const rem = (value: string | number) =>
    typeof value === "number" ? `${value / 16}rem` : value;
export const em = (value: string | number) =>
    typeof value === "number" ? `${value / 16}em` : value;
export const px = (value: string | number) =>
    typeof value === "number" ? value : parseFloat(value as string) * 16;

export { notify, type NotifyOptions, type NotifyTheme } from "./toaster";
export { ToasterHost } from "./ToasterHost";

export {
    confirm,
    ConfirmModalHost,
    type ConfirmOptions,
    type ConfirmTheme,
} from "./confirm-modal";

// =====================================================================
// === DEPRECATED legacy APIs — to be removed in a follow-up sweep ===
// =====================================================================
// `notifications` and `modals` exist purely so existing call sites still
// compile. New code MUST use `notify(...)` and `confirm(...)`.
import type { ReactNode } from "react";
import { notify } from "./toaster";
import { confirm } from "./confirm-modal";

type LegacyColor = "red" | "green" | "yellow" | "blue" | "default";
const legacyColorToTheme = (c?: LegacyColor): "info" | "success" | "warning" | "danger" | "info" => {
    if (c === "red") return "danger";
    if (c === "green") return "success";
    if (c === "yellow") return "warning";
    return "info";
};
const reactNodeToString = (n?: ReactNode): string | undefined => {
    if (n == null) return undefined;
    if (typeof n === "string") return n;
    if (typeof n === "number" || typeof n === "boolean") return String(n);
    return undefined;
};

/** @deprecated use `notify({ theme, title, content })` */
export const notifications = {
    show: (props: {
        title?: ReactNode;
        message: ReactNode;
        color?: LegacyColor;
        autoClose?: boolean | number;
    }) =>
        notify({
            title: reactNodeToString(props.title),
            content: props.message,
            theme: legacyColorToTheme(props.color),
            autoHiding:
                props.autoClose === false
                    ? false
                    : typeof props.autoClose === "number"
                      ? props.autoClose
                      : 5000,
        }),
    clean: () => notify.clear(),
};

interface LegacyConfirmOptions {
    title?: ReactNode;
    children?: ReactNode;
    message?: ReactNode;
    labels?: { confirm?: string; cancel?: string };
    confirmProps?: { color?: "red" | "blue" | "green" };
    onConfirm?: () => void;
    onCancel?: () => void;
}
const legacyButtonColor = (c?: "red" | "blue" | "green") => {
    if (c === "red") return "danger";
    if (c === "green") return "success";
    if (c === "blue") return "info";
    return "info";
};

/** @deprecated use `confirm({ title, message, theme, confirmText, onConfirm })` */
export const modals = {
    openConfirmModal: (opts: LegacyConfirmOptions) =>
        confirm({
            title: reactNodeToString(opts.title),
            message: opts.message ?? opts.children,
            confirmText: opts.labels?.confirm,
            cancelText: opts.labels?.cancel,
            theme: legacyButtonColor(opts.confirmProps?.color),
            onConfirm: opts.onConfirm,
            onCancel: opts.onCancel,
        }),
    confirm: (opts: LegacyConfirmOptions) =>
        confirm({
            title: reactNodeToString(opts.title),
            message: opts.message ?? opts.children,
            confirmText: opts.labels?.confirm,
            cancelText: opts.labels?.cancel,
            theme: legacyButtonColor(opts.confirmProps?.color),
            onConfirm: opts.onConfirm,
            onCancel: opts.onCancel,
        }),
    closeAll: () => confirm.closeAll(),
    close: (id: string) => confirm.close(id),
};

/** @deprecated use `ConfirmOptions` */
export type OpenConfirmModalOptions = LegacyConfirmOptions;

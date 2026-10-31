import { useEffect, useState } from "react";
import { CheckCircle2, CircleAlert, Info, TriangleAlert, X } from "lucide-react";
import { Portal } from "../ui/Portal";
import styles from "./ToasterHost.module.css";
import { type NotifyTheme, type Toast, toastStore } from "./toaster";

const ICONS: Record<NotifyTheme, typeof Info> = {
    info: Info,
    success: CheckCircle2,
    warning: TriangleAlert,
    danger: CircleAlert,
};

function ToastCard({ toast }: { toast: Toast }) {
    useEffect(() => {
        if (toast.autoHiding === false) return;
        const ms = typeof toast.autoHiding === "number" ? toast.autoHiding : 5000;
        const timer = setTimeout(() => toastStore.remove(toast.name), ms);
        return () => clearTimeout(timer);
    }, [toast.name, toast.autoHiding]);

    const Icon = ICONS[toast.theme];
    return (
        <div className={styles.toast} data-theme={toast.theme} role="status">
            <span className={styles.iconWrap}>
                <Icon size={15} />
            </span>
            <div className={styles.body}>
                {toast.title != null && <div className={styles.title}>{toast.title}</div>}
                {toast.content != null && <div className={styles.content}>{toast.content}</div>}
            </div>
            {toast.action && (
                <button
                    type="button"
                    className={styles.action}
                    onClick={() => {
                        toastStore.remove(toast.name);
                        toast.action?.onClick();
                    }}
                >
                    {toast.action.label}
                </button>
            )}
            <button
                type="button"
                className={styles.close}
                aria-label="Dismiss"
                onClick={() => toastStore.remove(toast.name)}
            >
                <X size={14} />
            </button>
        </div>
    );
}

/** Mounted once by `UIProvider`; renders toasts queued via `notify()`. */
export function ToasterHost() {
    const [toasts, setToasts] = useState<Toast[]>([]);
    useEffect(() => toastStore.subscribe(setToasts), []);

    if (toasts.length === 0) return null;
    return (
        <Portal>
            <div className={styles.host}>
                {toasts.map((toast) => (
                    <ToastCard key={toast.name} toast={toast} />
                ))}
            </div>
        </Portal>
    );
}

import {
  createSignal,
  createContext,
  useContext,
  For,
  Show,
  type ParentComponent,
  type Component,
  onCleanup,
} from "solid-js";
import { Portal } from "solid-js/web";
import { IconCheck, IconX, IconAlert, IconInfo } from "../Icon";
import styles from "./Toast.module.css";

export type ToastType = "success" | "error" | "warning" | "info";

export interface Toast {
  id: string;
  title?: string;
  message: string;
  type: ToastType;
  duration?: number;
}

interface ToastContextValue {
  show: (options: Omit<Toast, "id">) => void;
  success: (message: string, title?: string) => void;
  error: (message: string, title?: string) => void;
  warning: (message: string, title?: string) => void;
  info: (message: string, title?: string) => void;
  dismiss: (id: string) => void;
}

const ToastContext = createContext<ToastContextValue>();

let toastId = 0;
const generateId = () => `toast-${++toastId}`;

export const ToastProvider: ParentComponent = (props) => {
  const [toasts, setToasts] = createSignal<Toast[]>([]);

  const dismiss = (id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  };

  const show = (options: Omit<Toast, "id">) => {
    const id = generateId();
    const toast: Toast = { ...options, id };
    setToasts((prev) => [...prev, toast]);

    // Auto dismiss
    const duration = options.duration ?? 5000;
    if (duration > 0) {
      setTimeout(() => dismiss(id), duration);
    }
  };

  const success = (message: string, title?: string) =>
    show({ type: "success", message, title });
  const error = (message: string, title?: string) =>
    show({ type: "error", message, title, duration: 0 }); // Errors don't auto-dismiss
  const warning = (message: string, title?: string) =>
    show({ type: "warning", message, title });
  const info = (message: string, title?: string) =>
    show({ type: "info", message, title });

  const value: ToastContextValue = {
    show,
    success,
    error,
    warning,
    info,
    dismiss,
  };

  return (
    <ToastContext.Provider value={value}>
      {props.children}
      <Portal>
        <div class={styles.container}>
          <For each={toasts()}>
            {(toast) => (
              <ToastItem toast={toast} onDismiss={() => dismiss(toast.id)} />
            )}
          </For>
        </div>
      </Portal>
    </ToastContext.Provider>
  );
};

interface ToastItemProps {
  toast: Toast;
  onDismiss: () => void;
}

const ToastItem: Component<ToastItemProps> = (props) => {
  const IconComponent = {
    success: IconCheck,
    error: IconX,
    warning: IconAlert,
    info: IconInfo,
  }[props.toast.type];

  return (
    <div class={`${styles.toast} ${styles[props.toast.type]}`}>
      <div class={styles.iconWrapper}>
        <IconComponent size={18} />
      </div>
      <div class={styles.content}>
        <Show when={props.toast.title}>
          <div class={styles.title}>{props.toast.title}</div>
        </Show>
        <div class={styles.message}>{props.toast.message}</div>
      </div>
      <button class={styles.closeButton} onClick={props.onDismiss}>
        <IconX size={16} />
      </button>
    </div>
  );
};

export const useToast = (): ToastContextValue => {
  const context = useContext(ToastContext);
  if (!context) {
    throw new Error("useToast must be used within ToastProvider");
  }
  return context;
};

// Global toast instance for use outside of components
let globalToast: ToastContextValue | null = null;

export const setGlobalToast = (toast: ToastContextValue) => {
  globalToast = toast;
};

export const toast = {
  show: (options: Omit<Toast, "id">) => globalToast?.show(options),
  success: (message: string, title?: string) => globalToast?.success(message, title),
  error: (message: string, title?: string) => globalToast?.error(message, title),
  warning: (message: string, title?: string) => globalToast?.warning(message, title),
  info: (message: string, title?: string) => globalToast?.info(message, title),
};

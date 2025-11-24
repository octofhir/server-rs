import { type ParentComponent, Show, createEffect, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import styles from "./Modal.module.css";

export interface ModalProps {
  open: boolean;
  onClose: () => void;
  title?: string;
  size?: "sm" | "md" | "lg";
  class?: string;
}

export const Modal: ParentComponent<ModalProps> = (props) => {
  let contentRef: HTMLDivElement | undefined;

  createEffect(() => {
    if (props.open) {
      document.body.style.overflow = "hidden";
    } else {
      document.body.style.overflow = "";
    }
  });

  onCleanup(() => {
    document.body.style.overflow = "";
  });

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  const handleEscape = (e: KeyboardEvent) => {
    if (e.key === "Escape" && props.open) {
      props.onClose();
    }
  };

  createEffect(() => {
    if (props.open) {
      document.addEventListener("keydown", handleEscape);
    } else {
      document.removeEventListener("keydown", handleEscape);
    }
  });

  onCleanup(() => {
    document.removeEventListener("keydown", handleEscape);
  });

  return (
    <Show when={props.open}>
      <Portal>
        <div class={styles.backdrop} onClick={handleBackdropClick}>
          <div
            ref={contentRef}
            class={`${styles.modal} ${styles[props.size || "md"]} ${props.class || ""}`}
            role="dialog"
            aria-modal="true"
          >
            <Show when={props.title}>
              <div class={styles.header}>
                <h2 class={styles.title}>{props.title}</h2>
                <button
                  class={styles.closeButton}
                  onClick={props.onClose}
                  aria-label="Close"
                >
                  ×
                </button>
              </div>
            </Show>
            <div class={styles.content}>{props.children}</div>
          </div>
        </div>
      </Portal>
    </Show>
  );
};

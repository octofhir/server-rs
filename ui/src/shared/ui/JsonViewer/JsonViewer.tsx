import { createSignal, createMemo } from "solid-js";
import styles from "./JsonViewer.module.css";

export interface JsonViewerProps {
  data: unknown;
  class?: string;
}

export const JsonViewer = (props: JsonViewerProps) => {
  const formattedJson = createMemo(() => {
    try {
      return JSON.stringify(props.data, null, 2);
    } catch {
      return String(props.data);
    }
  });

  const [copied, setCopied] = createSignal(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(formattedJson());
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("Failed to copy:", err);
    }
  };

  return (
    <div class={`${styles.container} ${props.class || ""}`}>
      <div class={styles.toolbar}>
        <button class={styles.copyButton} onClick={handleCopy} type="button">
          {copied() ? "Copied!" : "Copy"}
        </button>
      </div>
      <pre class={styles.code}>
        <code>{formattedJson()}</code>
      </pre>
    </div>
  );
};

import { createSignal, onCleanup, onMount, type JSX } from "solid-js";
import styles from "./Splitter.module.css";

export interface SplitterProps {
  direction: "horizontal" | "vertical";
  defaultSize?: number;
  minSize?: number;
  maxSize?: number;
  onResize?: (size: number) => void;
  disabled?: boolean;
  class?: string;
  children: [JSX.Element, JSX.Element];
}

export const Splitter = (props: SplitterProps) => {
  const [size, setSize] = createSignal(props.defaultSize ?? 50);
  const [isDragging, setIsDragging] = createSignal(false);
  let containerRef: HTMLDivElement | undefined;
  let startPosition = 0;
  let startSize = 0;

  const minSize = () => props.minSize ?? 10;
  const maxSize = () => props.maxSize ?? 90;
  const isHorizontal = () => props.direction === "horizontal";

  const getContainerSize = () => {
    if (!containerRef) return 0;
    return isHorizontal() ? containerRef.offsetWidth : containerRef.offsetHeight;
  };

  const updateSize = (newSize: number) => {
    const constrained = Math.min(Math.max(newSize, minSize()), maxSize());
    setSize(constrained);
    props.onResize?.(constrained);
  };

  const handleMouseDown = (e: MouseEvent) => {
    if (props.disabled) return;
    e.preventDefault();
    setIsDragging(true);
    startPosition = isHorizontal() ? e.clientX : e.clientY;
    startSize = (size() / 100) * getContainerSize();
    document.body.style.userSelect = "none";
    document.body.style.cursor = isHorizontal() ? "col-resize" : "row-resize";
  };

  const handleMouseMove = (e: MouseEvent) => {
    if (!isDragging()) return;
    const currentPos = isHorizontal() ? e.clientX : e.clientY;
    const delta = currentPos - startPosition;
    const newSizePixels = startSize + delta;
    const containerSize = getContainerSize();
    if (containerSize > 0) {
      updateSize((newSizePixels / containerSize) * 100);
    }
  };

  const handleMouseUp = () => {
    if (isDragging()) {
      setIsDragging(false);
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
    }
  };

  onMount(() => {
    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
  });

  onCleanup(() => {
    document.removeEventListener("mousemove", handleMouseMove);
    document.removeEventListener("mouseup", handleMouseUp);
  });

  return (
    <div
      ref={containerRef}
      class={`${styles.container} ${styles[props.direction]} ${props.class || ""}`}
    >
      <div
        class={styles.panel}
        style={{ [isHorizontal() ? "width" : "height"]: `${size()}%` }}
      >
        {props.children[0]}
      </div>

      <button
        type="button"
        class={`${styles.resizer} ${isDragging() ? styles.dragging : ""} ${props.disabled ? styles.disabled : ""}`}
        onMouseDown={handleMouseDown}
        aria-label={`Resize ${props.direction} splitter`}
        disabled={props.disabled}
      >
        <span class={styles.resizerHandle} />
      </button>

      <div
        class={styles.panel}
        style={{ [isHorizontal() ? "width" : "height"]: `${100 - size()}%` }}
      >
        {props.children[1]}
      </div>
    </div>
  );
};

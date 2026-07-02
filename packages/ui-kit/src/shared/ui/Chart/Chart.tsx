import { type CSSProperties, forwardRef, useEffect, useImperativeHandle, useRef } from "react";

import styles from "./Chart.module.css";
import { type EChartsCoreOption, type EChartsType, echarts } from "./echarts";
import { useChartTheme } from "./useChartTheme";

export type ChartRenderer = "canvas" | "svg";

export interface ChartProps {
  /** ECharts option object. Re-applied via `setOption` whenever it changes. */
  option: EChartsCoreOption;
  /** Chart height in px (or any CSS length). Defaults to 320. */
  height?: number | string;
  /** Optional explicit width; defaults to filling the container. */
  width?: number | string;
  /** Rendering backend. Changing it re-initializes the instance. */
  renderer?: ChartRenderer;
  /**
   * Theme name (registered) or inline theme object. Defaults to the active
   * brand theme resolved from the color scheme. Changing it re-initializes.
   */
  theme?: string | object;
  /** Replace the whole option instead of merging. */
  notMerge?: boolean;
  /** Lazily apply the option update. Defaults to true. */
  lazyUpdate?: boolean;
  /** Show the ECharts loading spinner. */
  loading?: boolean;
  /** Event handlers bound via `instance.on(name, handler)`. */
  onEvents?: Record<string, (params: unknown, instance: EChartsType) => void>;
  /** Called with the instance right after init. */
  onInit?: (instance: EChartsType) => void;
  /** Connect charts for shared tooltip/datazoom via `echarts.connect`. */
  group?: string;
  className?: string;
  style?: CSSProperties;
  "aria-label"?: string;
}

/**
 * Imperative ECharts wrapper. Owns the instance lifecycle (init / setOption /
 * resize / dispose) and applies the brand theme by default. The underlying
 * `EChartsType` instance is exposed through the forwarded ref.
 */
export const Chart = forwardRef<EChartsType | null, ChartProps>(function Chart(
  {
    option,
    height = 320,
    width,
    renderer = "canvas",
    theme,
    notMerge,
    lazyUpdate = true,
    loading,
    onEvents,
    onInit,
    group,
    className,
    style,
    "aria-label": ariaLabel,
  },
  ref
) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const instanceRef = useRef<EChartsType | null>(null);
  const onInitRef = useRef(onInit);
  onInitRef.current = onInit;

  const defaultTheme = useChartTheme();
  const activeTheme = theme ?? defaultTheme;

  useImperativeHandle<EChartsType | null, EChartsType | null>(ref, () => instanceRef.current, []);

  // Init (and re-init on renderer / theme / group change).
  useEffect(() => {
    const dom = containerRef.current;
    if (!dom) return;

    // useDirtyRect is intentionally off: it leaves trails / flicker on pie
    // charts and label lines (known ECharts artifact).
    const instance = echarts.init(dom, activeTheme, { renderer });
    instanceRef.current = instance;
    if (group) instance.group = group;
    onInitRef.current?.(instance);

    return () => {
      instance.dispose();
      instanceRef.current = null;
    };
  }, [renderer, activeTheme, group]);

  // Apply option.
  useEffect(() => {
    instanceRef.current?.setOption(option, { notMerge, lazyUpdate });
  }, [option, notMerge, lazyUpdate]);

  // Loading state.
  useEffect(() => {
    const instance = instanceRef.current;
    if (!instance) return;
    if (loading) instance.showLoading();
    else instance.hideLoading();
  }, [loading]);

  // Bind events.
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-bind only when handlers or theme/renderer (re-init) change.
  useEffect(() => {
    const instance = instanceRef.current;
    if (!instance || !onEvents) return;
    const entries = Object.entries(onEvents);
    for (const [name, handler] of entries) {
      instance.on(name, (params: unknown) => handler(params, instance));
    }
    return () => {
      for (const [name] of entries) {
        instance.off(name);
      }
    };
  }, [onEvents, renderer, activeTheme]);

  // Resize observer → rAF-debounced resize.
  useEffect(() => {
    const dom = containerRef.current;
    if (!dom) return;

    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        instanceRef.current?.resize();
      });
    });
    observer.observe(dom);

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, []);

  return (
    <div
      ref={containerRef}
      className={className ? `${styles.root} ${className}` : styles.root}
      role="img"
      aria-label={ariaLabel}
      style={{ height, ...(width != null ? { width } : null), ...style }}
    />
  );
});

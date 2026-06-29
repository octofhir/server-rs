import { forwardRef, type HTMLAttributes, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export interface MarkdownViewProps extends HTMLAttributes<HTMLDivElement> {
  /** Raw GFM markdown source. */
  children?: string;
  /** Alias for `children` when passing as a prop is more convenient. */
  source?: string;
  /**
   * Optional variable scope. Any `${name}` (or `${name.path.to.value}`) token in the
   * source is substituted with the matching value before rendering. Missing names are
   * left intact so authors can see the unresolved reference.
   */
  scope?: Record<string, unknown>;
}

const TOKEN = /\$\{([a-zA-Z_][\w.[\]]*)\}/g;

function resolvePath(scope: Record<string, unknown>, path: string): unknown {
  const parts = path.replace(/\[(\d+)\]/g, ".$1").split(".");
  let cur: unknown = scope;
  for (const p of parts) {
    if (cur == null || typeof cur !== "object") return undefined;
    cur = (cur as Record<string, unknown>)[p];
  }
  return cur;
}

function interpolate(src: string, scope?: Record<string, unknown>): string {
  if (!scope) return src;
  return src.replace(TOKEN, (whole, path: string) => {
    const v = resolvePath(scope, path);
    if (v === undefined) return whole;
    return typeof v === "object" ? JSON.stringify(v) : String(v);
  });
}

/**
 * GFM markdown renderer for OctoFHIR. Tables, task lists, strikethrough, autolinks.
 * Themed via `--octo-*` tokens. Supports `${var}` interpolation from a scope map so
 * notebook prose can embed live values.
 */
export const MarkdownView = forwardRef<HTMLDivElement, MarkdownViewProps>(
  ({ children, source, scope, className, style, ...rest }, ref) => {
    const raw = source ?? children ?? "";
    const text = useMemo(() => interpolate(raw, scope), [raw, scope]);
    return (
      <div
        ref={ref}
        className={className ? `octo-markdown ${className}` : "octo-markdown"}
        style={style}
        {...rest}
      >
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
      </div>
    );
  }
);
MarkdownView.displayName = "MarkdownView";

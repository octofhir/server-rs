import { Check, ChevronDown, ChevronRight, Copy } from "lucide-react";
import { useState } from "react";
import classes from "../CqlConsolePage.module.css";
import type { CqlDatatype, CqlDefine } from "../types";

interface Props {
  define: CqlDefine;
  /** Hide the define name (single-expression mode). */
  hideName?: boolean;
}

export function ResultItem({ define, hideName }: Props) {
  const isComplex = typeof define.value === "object" && define.value !== null;
  const [expanded, setExpanded] = useState(isComplex);
  const [copied, setCopied] = useState(false);

  const copyText = isComplex
    ? JSON.stringify(define.value, null, 2)
    : formatPrimitiveValue(define.value);

  const handleCopy = () => {
    void navigator.clipboard.writeText(copyText).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    });
  };

  return (
    <div className={classes.resultItem}>
      <div className={classes.resultItemContent}>
        <div className={classes.resultHeader}>
          {!hideName && <span className={classes.defineName}>{define.name}</span>}
          <span className={`${classes.typeTag} ${typeClass(define.datatype)}`}>
            {define.datatype}
          </span>

          {isComplex ? (
            <button
              type="button"
              onClick={() => setExpanded(!expanded)}
              className={classes.resultToggle}
              aria-expanded={expanded}
              aria-label={`${expanded ? "Collapse" : "Expand"} ${define.name}`}
            >
              {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              {Array.isArray(define.value)
                ? `list (${define.value.length})`
                : `${define.datatype} object`}
            </button>
          ) : (
            <span className={classes.resultValue}>{formatPrimitiveValue(define.value)}</span>
          )}

          <button
            type="button"
            className={classes.copyBtn}
            onClick={handleCopy}
            aria-label="Copy value"
            title="Copy value"
          >
            {copied ? <Check size={14} /> : <Copy size={14} />}
          </button>
        </div>

        {isComplex && expanded && (
          <pre className={classes.resultJson}>{JSON.stringify(define.value, null, 2)}</pre>
        )}
      </div>
    </div>
  );
}

function typeClass(datatype: CqlDatatype): string {
  if (datatype === "string") return classes.typeString;
  if (datatype === "integer" || datatype === "decimal") return classes.typeNumber;
  if (datatype === "boolean") return classes.typeBool;
  if (datatype === "date" || datatype === "dateTime" || datatype === "time")
    return classes.typeDate;
  return classes.typeComplex;
}

function formatPrimitiveValue(value: unknown): string {
  if (value === null || value === undefined) return "null";
  if (typeof value === "string") return `"${value}"`;
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return value.toString();
  return String(value);
}

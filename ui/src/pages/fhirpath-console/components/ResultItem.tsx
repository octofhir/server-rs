import { Check, ChevronDown, ChevronRight, Copy } from "lucide-react";
import { useState } from "react";
import classes from "../FhirPathConsolePage.module.css";
import type { FhirPathResult } from "../types";

interface Props {
  result: FhirPathResult;
}

export function ResultItem({ result }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const isComplex = typeof result.value === "object" && result.value !== null;

  const copyText = isComplex
    ? JSON.stringify(result.value, null, 2)
    : formatPrimitiveValue(result.value);

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
          <span className={classes.resultIndex}>[{result.index}]</span>
          <span className={`${classes.typeTag} ${typeClass(result.datatype)}`}>
            {result.datatype}
          </span>

          {isComplex ? (
            <button
              type="button"
              onClick={() => setExpanded(!expanded)}
              className={classes.resultToggle}
              aria-expanded={expanded}
              aria-label={`${expanded ? "Collapse" : "Expand"} ${result.datatype} object`}
            >
              {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              {result.datatype} object
            </button>
          ) : (
            <span className={classes.resultValue}>{formatPrimitiveValue(result.value)}</span>
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
          <pre className={classes.resultJson}>{JSON.stringify(result.value, null, 2)}</pre>
        )}
      </div>
    </div>
  );
}

function typeClass(datatype: string): string {
  if (
    datatype === "string" ||
    datatype === "code" ||
    datatype === "id" ||
    datatype === "uri" ||
    datatype === "url"
  )
    return classes.typeString;
  if (datatype === "integer" || datatype === "decimal") return classes.typeNumber;
  if (datatype === "boolean") return classes.typeBool;
  if (datatype === "date" || datatype === "dateTime" || datatype === "time")
    return classes.typeDate;
  return classes.typeComplex;
}

function formatPrimitiveValue(value: unknown): string {
  if (typeof value === "string") return `"${value}"`;
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return value.toString();
  return String(value);
}

import type { ReactNode } from "react";

const C_KEY = { color: "var(--octo-accent-primary)" };
const C_STRING = { color: "var(--octo-accent-positive, #059669)" };
const C_BOOL = { color: "var(--octo-accent-warm, #b45309)" };
const C_NULL = { color: "var(--octo-text-secondary)" };
const C_NUMBER = { color: "var(--octo-accent-primary, #1d4ed8)" };

/** Token-colour a JSON string for read-only display. Shared by cell + raw views. */
export function highlightJson(json: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const regex =
    /("(?:\\.|[^"\\])*")\s*:|("(?:\\.|[^"\\])*")|(true|false)|(null)|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g;
  let lastIndex = 0;
  let key = 0;

  for (let match = regex.exec(json); match !== null; match = regex.exec(json)) {
    if (match.index > lastIndex) {
      nodes.push(json.slice(lastIndex, match.index));
    }

    if (match[1] !== undefined) {
      nodes.push(
        <span key={key++} style={C_KEY}>
          {match[1]}
        </span>
      );
      nodes.push(": ");
    } else if (match[2] !== undefined) {
      nodes.push(
        <span key={key++} style={C_STRING}>
          {match[2]}
        </span>
      );
    } else if (match[3] !== undefined) {
      nodes.push(
        <span key={key++} style={C_BOOL}>
          {match[3]}
        </span>
      );
    } else if (match[4] !== undefined) {
      nodes.push(
        <span key={key++} style={C_NULL}>
          {match[4]}
        </span>
      );
    } else if (match[5] !== undefined) {
      nodes.push(
        <span key={key++} style={C_NUMBER}>
          {match[5]}
        </span>
      );
    }

    lastIndex = match.index + match[0].length;
  }

  if (lastIndex < json.length) {
    nodes.push(json.slice(lastIndex));
  }

  return nodes;
}

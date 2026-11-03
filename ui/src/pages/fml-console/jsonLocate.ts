/**
 * Where a value is in JSON text: the range of the value a trace variable's `path`
 * (`Patient.name[1].given[0]`) names, for highlighting it in the input editor.
 */

interface Span {
  start: number;
  end: number;
  members?: Map<string, Span>;
  items?: Span[];
}

/** Parses JSON text into value spans; `undefined` when it is not JSON. */
function parse(text: string): Span | undefined {
  let at = 0;
  const space = () => {
    while (at < text.length && /\s/.test(text[at])) at++;
  };
  const string = (): string => {
    const start = at;
    at++; // opening quote
    while (at < text.length && text[at] !== '"') at += text[at] === "\\" ? 2 : 1;
    at++; // closing quote
    return JSON.parse(text.slice(start, at)) as string;
  };
  const value = (): Span => {
    space();
    const start = at;
    const c = text[at];
    if (c === "{") {
      at++;
      const members = new Map<string, Span>();
      space();
      if (text[at] === "}") {
        at++;
        return { start, end: at, members };
      }
      for (;;) {
        space();
        const key = string();
        space();
        if (text[at] !== ":") throw new Error("expected :");
        at++;
        members.set(key, value());
        space();
        if (text[at] === ",") {
          at++;
          continue;
        }
        if (text[at] !== "}") throw new Error("expected }");
        at++;
        return { start, end: at, members };
      }
    }
    if (c === "[") {
      at++;
      const items: Span[] = [];
      space();
      if (text[at] === "]") {
        at++;
        return { start, end: at, items };
      }
      for (;;) {
        items.push(value());
        space();
        if (text[at] === ",") {
          at++;
          continue;
        }
        if (text[at] !== "]") throw new Error("expected ]");
        at++;
        return { start, end: at, items };
      }
    }
    if (c === '"') {
      string();
      return { start, end: at };
    }
    while (at < text.length && /[^\s,\]}]/.test(text[at])) at++;
    if (at === start) throw new Error("unexpected character");
    return { start, end: at };
  };
  try {
    return value();
  } catch {
    return undefined;
  }
}

/** The member a path segment names: the name itself, or a choice (`value` → `valueString`). */
function member(members: Map<string, Span>, name: string): Span | undefined {
  const exact = members.get(name);
  if (exact) return exact;
  for (const [key, span] of members) {
    if (key.startsWith(name) && /^[A-Z]/.test(key.slice(name.length))) return span;
  }
  return undefined;
}

/** The text range of the value at `path` (its first segment, the resource type, is the root). */
export function locate(text: string, path: string): { start: number; end: number } | undefined {
  let span = parse(text);
  if (!span) return undefined;
  for (const segment of path.split(".").slice(1)) {
    const match = /^([^[\]]+)(?:\[(\d+)\])?$/.exec(segment);
    if (!match || !span.members) return undefined;
    let next = member(span.members, match[1]);
    if (next?.items) next = next.items[Number(match[2] ?? 0)];
    if (!next) return undefined;
    span = next;
  }
  return { start: span.start, end: span.end };
}

import {
  Boxes,
  ChevronDown,
  Code2,
  Database,
  FileText,
  FolderTree,
  KeyRound,
  Moon,
  NotebookPen,
  Play,
  Plus,
  Search,
  Send,
  Settings,
  Shield,
  Sigma,
  Table,
  Terminal,
  Users,
} from "lucide-react";
import { type CSSProperties, Fragment, useEffect, useRef, useState } from "react";
import classes from "./styleguide.module.css";

/**
 * Dev-only design direction specimens. Everything here is a static mock styled by
 * per-direction CSS variables so light and dark can sit side by side; the chosen
 * direction is later rolled into the ui-kit tokens.
 */

type DirectionId = "terminal" | "studio" | "ink";
type Scheme = "light" | "dark";

const DIRECTIONS: { id: DirectionId; name: string; summary: string }[] = [
  {
    id: "terminal",
    name: "A · Terminal",
    summary: "Dense: 28px rows, 4–6px radii, mono headings and labels, hairlines.",
  },
  {
    id: "studio",
    name: "B · Studio",
    summary: "Calm: Inter, 38px rows, 8px radii, sidebar shares the page surface.",
  },
  {
    id: "ink",
    name: "C · Ink rail",
    summary: "Ink sidebar in both themes, Marigold only on actions, 32px rows.",
  },
];

const NAV = [
  {
    label: "Data",
    items: [
      { icon: FolderTree, label: "Resources", active: true },
      { icon: Send, label: "Requests" },
      { icon: Boxes, label: "Packages" },
    ],
  },
  {
    label: "Query",
    items: [
      { icon: Database, label: "SQL" },
      { icon: Code2, label: "GraphQL" },
      { icon: Terminal, label: "FHIRPath" },
      { icon: Sigma, label: "CQL" },
      { icon: Table, label: "Views" },
      { icon: NotebookPen, label: "Notebooks" },
    ],
  },
  {
    label: "Access",
    items: [
      { icon: Users, label: "Users" },
      { icon: KeyRound, label: "Clients" },
      { icon: Shield, label: "Policies" },
    ],
  },
  {
    label: "System",
    items: [
      { icon: FileText, label: "Logs" },
      { icon: Settings, label: "Settings" },
    ],
  },
];

const PATIENTS = [
  ["01a18e53", "Anna Petrova", "female", "1984-03-12", "2m ago", "active"],
  ["4c414635", "John Smith", "male", "1971-11-02", "2m ago", "active"],
  ["4c78f734", "Maria Garcia", "female", "1990-07-21", "5m ago", "active"],
  ["4ca19f5d", "Liam Chen", "male", "2001-01-30", "1h ago", "inactive"],
  ["4cdda8cc", "Olga Ivanova", "female", "1958-09-14", "1h ago", "active"],
  ["4d0a469d", "Noah Brown", "male", "1966-05-05", "3h ago", "error"],
  ["4d3d53ce", "Emma Wilson", "female", "1999-12-24", "1d ago", "active"],
  ["4d6bb8a2", "Ivan Sidorov", "male", "1979-02-17", "2d ago", "draft"],
] as const;

const SWATCHES = [
  { token: "accent", label: "Accent fill" },
  { token: "on-accent", label: "On accent", on: "accent" },
  { token: "accent-text", label: "Accent text", text: true },
  { token: "accent-soft", label: "Accent soft" },
  { token: "bg", label: "Page" },
  { token: "surface", label: "Surface" },
  { token: "surface-2", label: "Surface 2" },
  { token: "border", label: "Border" },
  { token: "text", label: "Text", text: true },
  { token: "text-2", label: "Secondary", text: true },
  { token: "text-3", label: "Muted", text: true },
  { token: "ok", label: "Success", text: true },
  { token: "warn", label: "Warning", text: true },
  { token: "err", label: "Error", text: true },
  { token: "info", label: "Info", text: true },
];

export function StyleguidePage() {
  const [direction, setDirection] = useState<DirectionId>(() => {
    const fromUrl = new URLSearchParams(window.location.search).get("dir");
    return DIRECTIONS.some((d) => d.id === fromUrl) ? (fromUrl as DirectionId) : "terminal";
  });

  return (
    <div className={classes.page}>
      <header className={classes.top}>
        <img src={`${import.meta.env.BASE_URL}branding/logo-dark.svg`} alt="OctoFHIR" height={22} />
        <span className={classes.topTitle}>Styleguide · design directions</span>
        <nav className={classes.tabs}>
          {DIRECTIONS.map((d) => (
            <button
              key={d.id}
              type="button"
              className={classes.tab}
              data-active={d.id === direction}
              onClick={() => setDirection(d.id)}
            >
              {d.name}
            </button>
          ))}
        </nav>
      </header>
      <p className={classes.summary}>{DIRECTIONS.find((d) => d.id === direction)?.summary}</p>

      {(["light", "dark"] as const).map((scheme) => (
        <section key={scheme} className={classes.dir} data-dir={direction} data-scheme={scheme}>
          <h2 className={classes.schemeLabel}>{scheme}</h2>
          <ShellSpecimen scheme={scheme} direction={direction} />
          <div className={classes.split}>
            <ConsoleSpecimen />
            <div className={classes.stack}>
              <TokenSpecimen />
              <TypeSpecimen />
            </div>
          </div>
        </section>
      ))}
    </div>
  );
}

function ShellSpecimen({ scheme, direction }: { scheme: Scheme; direction: DirectionId }) {
  const logo = direction === "ink" || scheme === "dark" ? "logo-dark" : "logo-light";
  return (
    <div className={classes.shell}>
      <aside className={classes.side}>
        <img
          className={classes.brand}
          src={`${import.meta.env.BASE_URL}branding/${logo}.svg`}
          alt="OctoFHIR"
        />
        <button type="button" className={classes.jump}>
          <Search size={14} />
          <span>Jump to…</span>
          <kbd>⌘K</kbd>
        </button>
        {NAV.map((group) => (
          <div key={group.label} className={classes.group}>
            <div className={classes.groupLabel}>{group.label}</div>
            {group.items.map(({ icon: Icon, label, active }) => (
              <div key={label} className={classes.navItem} data-active={active ?? false}>
                <Icon size={15} />
                {label}
              </div>
            ))}
          </div>
        ))}
        <div className={classes.sideFoot}>
          <span className={classes.health} /> 0.0.1 · healthy
        </div>
      </aside>
      <main className={classes.main}>
        <div className={classes.crumbs}>
          <span className={classes.path}>
            fhir<i>/</i>
            <b>Patient</b>
          </span>
          <span className={classes.crumbMeta}>R4 · 8 resources</span>
          <span className={classes.headerActions}>
            <button type="button" className={classes.iconBtn} aria-label="Theme">
              <Moon size={14} />
            </button>
            <span className={classes.avatar}>A</span>
          </span>
        </div>
        <div className={classes.content}>
          <div className={classes.titleRow}>
            <h1 className={classes.h1}>Patient</h1>
            <span className={classes.count}>8</span>
            <span className={classes.spacer} />
            <button type="button" className={classes.btnGhost}>
              Export
            </button>
            <button type="button" className={classes.btnPrimary}>
              <Plus size={14} /> New patient
            </button>
          </div>
          <div className={classes.toolbar}>
            <div className={classes.query}>
              <Search size={14} />
              <span>
                name=<b>anna</b>&amp;_sort=<b>-birthdate</b>
              </span>
            </div>
            <button type="button" className={classes.filter}>
              gender <ChevronDown size={12} />
            </button>
            <button type="button" className={classes.filter}>
              active <ChevronDown size={12} />
            </button>
          </div>
          <table className={classes.table}>
            <thead>
              <tr>
                <th>id</th>
                <th>Name</th>
                <th>Gender</th>
                <th>Birth date</th>
                <th>Updated</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              {PATIENTS.map(([id, name, gender, birth, updated, status], i) => (
                <tr key={id} data-selected={i === 1}>
                  <td>
                    <span className={classes.chip}>
                      <i>Patient/</i>
                      {id}
                    </span>
                  </td>
                  <td className={classes.strong}>{name}</td>
                  <td>{gender}</td>
                  <td className={classes.mono}>{birth}</td>
                  <td className={classes.muted}>{updated}</td>
                  <td>
                    <span className={classes.badge} data-tone={status}>
                      {status}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className={classes.pager}>
            <span>1–8 of 8</span>
            <span className={classes.spacer} />
            <kbd>j</kbd>
            <kbd>k</kbd> move · <kbd>↵</kbd> open
          </div>
        </div>
      </main>
    </div>
  );
}

const JSON_LINES: [string, string][][] = [
  [["p", "{"]],
  [["k", '  "resourceType"'], ["p", ": "], ["s", '"Bundle"'], ["p", ","]],
  [["k", '  "type"'], ["p", ": "], ["s", '"searchset"'], ["p", ","]],
  [["k", '  "total"'], ["p", ": "], ["n", "2"], ["p", ","]],
  [["k", '  "entry"'], ["p", ": [{"]],
  [["k", '    "fullUrl"'], ["p", ": "], ["s", '"Observation/7f2c91aa"'], ["p", ","]],
  [["k", '    "valueQuantity"'], ["p", ": { "], ["k", '"value"'], ["p", ": "], ["n", "63"], ["p", ", "], ["k", '"unit"'], ["p", ": "], ["s", '"/min"'], ["p", " }"]],
  [["p", "  }]"]],
  [["p", "}"]],
];

function ConsoleSpecimen() {
  return (
    <div className={classes.console}>
      <div className={classes.reqBar}>
        <span className={classes.method}>GET</span>
        <span className={classes.url}>
          /fhir/Observation?subject=<b>Patient/01a18e53</b>&amp;code=<b>8867-4</b>
        </span>
        <button type="button" className={classes.btnPrimary}>
          <Play size={13} /> Send <kbd>⌘↵</kbd>
        </button>
      </div>
      <div className={classes.resMeta}>
        <span className={classes.badge} data-tone="active">
          200 OK
        </span>
        <span>14 ms</span>
        <span>2.1 KB</span>
        <span className={classes.spacer} />
        <span className={classes.resTab} data-active="true">
          Body
        </span>
        <span className={classes.resTab}>Headers</span>
        <span className={classes.resTab}>Explain</span>
      </div>
      <pre className={classes.code}>
        {JSON_LINES.map((line, i) => (
          <Fragment key={i}>
            <span className={classes.ln}>{i + 1}</span>
            {line.map(([kind, text], j) => (
                  <span key={j} className={classes[`tok${kind.toUpperCase()}` as keyof typeof classes]}>
                {text}
              </span>
            ))}
            {"\n"}
          </Fragment>
        ))}
      </pre>
    </div>
  );
}

function TokenSpecimen() {
  const ref = useRef<HTMLDivElement>(null);
  const [ratios, setRatios] = useState<Record<string, string>>({});

  // Contrast is computed from the resolved colours so every direction is checked against WCAG AA.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const style = getComputedStyle(el);
    const bg = style.getPropertyValue("--sg-surface").trim();
    const accent = style.getPropertyValue("--sg-accent").trim();
    const next: Record<string, string> = {};
    for (const s of SWATCHES) {
      const fg = style.getPropertyValue(`--sg-${s.token}`).trim();
      if (s.on) next[s.token] = contrast(fg, style.getPropertyValue(`--sg-${s.on}`).trim() || accent);
      else if (s.text) next[s.token] = contrast(fg, bg);
    }
    setRatios(next);
  }, []);

  return (
    <div ref={ref} className={classes.card}>
      <div className={classes.cardTitle}>Colour tokens</div>
      <div className={classes.swatches}>
        {SWATCHES.map((s) => (
          <div key={s.token} className={classes.swatch}>
            <span
              className={classes.swatchChip}
              style={{ "--chip": `var(--sg-${s.token})` } as CSSProperties}
            />
            <span>{s.label}</span>
            {ratios[s.token] && (
              <span className={classes.ratio} data-pass={Number.parseFloat(ratios[s.token]) >= 4.5}>
                {ratios[s.token]}
              </span>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function TypeSpecimen() {
  return (
    <div className={classes.card}>
      <div className={classes.cardTitle}>Type · radius · density</div>
      <div className={classes.typeH1}>Resource browser</div>
      <div className={classes.typeH2}>Search parameters</div>
      <div className={classes.typeBody}>
        Body text for descriptions and forms. Calm, readable, one size for everything.
      </div>
      <div className={classes.typeLabel}>Section label</div>
      <div className={classes.mono}>Patient/01a18e53 · http://hl7.org/fhir/StructureDefinition/Patient</div>
      <div className={classes.radii}>
        <span data-r="sm">sm</span>
        <span data-r="md">md</span>
        <span data-r="lg">lg</span>
        <span className={classes.rowDemo}>row height</span>
      </div>
    </div>
  );
}

function contrast(a: string, b: string): string {
  const la = luminance(a);
  const lb = luminance(b);
  if (la === null || lb === null) return "";
  const ratio = (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
  return `${ratio.toFixed(1)}:1`;
}

function luminance(hex: string): number | null {
  const m = /^#([0-9a-f]{6})$/i.exec(hex);
  if (!m) return null;
  const n = Number.parseInt(m[1], 16);
  const channel = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel((n >> 16) & 255) + 0.7152 * channel((n >> 8) & 255) + 0.0722 * channel(n & 255);
}

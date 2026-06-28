import {
  Badge,
  Button,
  Loader,
  NumberInput,
  notifications,
  Select,
  Switch,
  Text,
  TextArea,
  TextInput,
  useColorScheme,
} from "@octofhir/ui-kit";
import {
  Activity,
  AlertTriangle,
  Boxes,
  Database,
  Gauge,
  Globe,
  HardDrive,
  Languages,
  Layers,
  Monitor,
  Palette,
  RefreshCw,
  ScrollText,
  Server,
  Settings as SettingsIcon,
  ShieldCheck,
  SlidersHorizontal,
  ToggleRight,
} from "lucide-react";
import { type ComponentType, type ReactNode, useEffect, useMemo, useState } from "react";
import { useUiSettings } from "@/shared";
import {
  type CategoryConfig,
  type ConfigCategory,
  useBuildInfo,
  useConfigCategory,
  useFeatureFlags,
  useFormatterSettings,
  useHealth,
  useReloadConfig,
  useSetConfigValue,
  useSettings,
  useToggleFeature,
} from "@/shared/api/hooks";
import { FormatterSettings } from "@/shared/settings/FormatterSettings";
import classes from "./SettingsPage.module.css";

// =============================================================================
// Value helpers
// =============================================================================

function str(obj: CategoryConfig | null | undefined, key: string, fallback = ""): string {
  const v = obj?.[key];
  return typeof v === "string" ? v : fallback;
}
function num(obj: CategoryConfig | null | undefined, key: string, fallback: number): number {
  const v = obj?.[key];
  return typeof v === "number" ? v : fallback;
}
function bool(obj: CategoryConfig | null | undefined, key: string, fallback = false): boolean {
  const v = obj?.[key];
  return typeof v === "boolean" ? v : fallback;
}
function subObject(obj: CategoryConfig | null | undefined, key: string): CategoryConfig {
  const v = obj?.[key];
  return v && typeof v === "object" && !Array.isArray(v) ? (v as CategoryConfig) : {};
}

// =============================================================================
// Committed inputs (avoid one PUT per keystroke)
// =============================================================================

function TextSetting({
  label,
  description,
  value,
  placeholder,
  onCommit,
}: {
  label: string;
  description?: string;
  value: string;
  placeholder?: string;
  onCommit: (next: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <TextInput
      label={label}
      description={description}
      value={draft}
      placeholder={placeholder}
      onChange={setDraft}
      onBlur={() => {
        if (draft !== value) onCommit(draft);
      }}
    />
  );
}

function NumberSetting({
  label,
  description,
  value,
  min,
  max,
  step,
  onCommit,
}: {
  label: string;
  description?: string;
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onCommit: (next: number) => void;
}) {
  const [draft, setDraft] = useState<number | null>(value);
  useEffect(() => setDraft(value), [value]);
  // NumberInput has no onBlur prop — debounce the commit instead.
  useEffect(() => {
    if (draft == null || draft === value) return;
    const t = setTimeout(() => onCommit(draft), 700);
    return () => clearTimeout(t);
  }, [draft, value, onCommit]);
  return (
    <NumberInput
      label={label}
      description={description}
      value={draft}
      min={min}
      max={max}
      step={step}
      onChange={setDraft}
    />
  );
}

function PackagesSetting({
  value,
  onCommit,
}: {
  value: string[];
  onCommit: (next: string[]) => void;
}) {
  const text = value.join("\n");
  const [draft, setDraft] = useState(text);
  useEffect(() => setDraft(text), [text]);
  return (
    <TextArea
      label="Packages to load"
      description="One package per line, e.g. hl7.fhir.r4.core#4.0.1"
      rows={6}
      value={draft}
      onChange={setDraft}
      onBlur={() => {
        const list = draft
          .split("\n")
          .map((l) => l.trim())
          .filter(Boolean);
        if (JSON.stringify(list) !== JSON.stringify(value)) onCommit(list);
      }}
    />
  );
}

// =============================================================================
// Layout primitives
// =============================================================================

function Card({
  icon,
  title,
  description,
  children,
}: {
  icon: ReactNode;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section className={classes.section}>
      <div className={classes.sectionHead}>
        <div className={classes.sectionTitleWrap}>
          <div className={classes.sectionTitleRow}>
            <span className={classes.sectionIcon}>{icon}</span>
            <span className={classes.sectionTitle}>{title}</span>
          </div>
          <span className={classes.sectionDesc}>{description}</span>
        </div>
      </div>
      <div className={classes.sectionBody}>{children}</div>
    </section>
  );
}

function RestartBanner() {
  return (
    <div className={classes.warnBanner}>
      <AlertTriangle size={15} />
      These settings are saved immediately but take effect only after a server restart.
    </div>
  );
}

function ReadOnlyBanner({ children }: { children: ReactNode }) {
  return (
    <div className={classes.warnBanner}>
      <AlertTriangle size={15} />
      {children}
    </div>
  );
}

function InfoRow({ label, value, muted }: { label: string; value: ReactNode; muted?: boolean }) {
  return (
    <div className={classes.infoRow}>
      <span className={classes.infoLabel}>{label}</span>
      <span className={`${classes.infoValue} ${muted ? classes.infoValueMuted : ""}`}>{value}</span>
    </div>
  );
}

// =============================================================================
// Group definitions
// =============================================================================

type GroupKind = "live" | "restart" | "local";
interface GroupDef {
  id: string;
  label: string;
  icon: ComponentType<{ size?: number }>;
  kind: GroupKind;
  /** true → only available to authenticated admins */
  admin: boolean;
}

const GROUP_SECTIONS: Array<{ label: string; items: GroupDef[] }> = [
  {
    label: "Server",
    items: [
      { id: "general", label: "General", icon: SettingsIcon, kind: "live", admin: true },
      { id: "search", label: "Search", icon: Gauge, kind: "live", admin: true },
      { id: "terminology", label: "Terminology", icon: Languages, kind: "live", admin: true },
      { id: "packages", label: "Packages", icon: Boxes, kind: "live", admin: true },
      { id: "cache", label: "Cache", icon: Layers, kind: "live", admin: true },
      { id: "observability", label: "Observability", icon: Globe, kind: "live", admin: true },
      { id: "dbconsole", label: "DB Console", icon: SlidersHorizontal, kind: "live", admin: true },
      { id: "features", label: "Feature flags", icon: ToggleRight, kind: "live", admin: true },
    ],
  },
  {
    label: "Infrastructure",
    items: [
      { id: "database", label: "Database", icon: Database, kind: "restart", admin: true },
      { id: "redis", label: "Redis", icon: HardDrive, kind: "restart", admin: true },
      { id: "server", label: "Server", icon: Server, kind: "restart", admin: true },
      { id: "validation", label: "Validation", icon: ShieldCheck, kind: "restart", admin: true },
    ],
  },
  {
    label: "This browser",
    items: [
      { id: "appearance", label: "Appearance", icon: Palette, kind: "local", admin: false },
      { id: "console", label: "Console & UI", icon: Monitor, kind: "local", admin: false },
    ],
  },
];

const railDotClass: Record<GroupKind, string> = {
  live: classes.railDotLive,
  restart: classes.railDotRestart,
  local: classes.railDotLocal,
};

// =============================================================================
// Option lists
// =============================================================================

const themeOptions = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "auto", label: "System" },
];
const logLevelOptions = ["trace", "debug", "info", "warn", "error", "off"].map((v) => ({
  value: v,
  label: v[0].toUpperCase() + v.slice(1),
}));
const logFormatOptions = [
  { value: "json", label: "JSON (structured)" },
  { value: "text", label: "Text (human)" },
];
const fhirVersionOptions = ["R4", "R4B", "R5", "R6"].map((v) => ({ value: v, label: v }));
const sqlModeOptions = [
  { value: "readonly", label: "Read-only (SELECT)" },
  { value: "readwrite", label: "Read/write (DML)" },
  { value: "admin", label: "Admin (DDL)" },
];

type ThemeValue = "light" | "dark" | "auto";
function isThemeValue(v: string | null): v is ThemeValue {
  return v === "light" || v === "dark" || v === "auto";
}

// =============================================================================
// Page
// =============================================================================

export function SettingsPage() {
  const {
    data: health,
    refetch: refetchHealth,
    isRefetching,
  } = useHealth({ refetchInterval: false });
  const { data: serverSettings } = useSettings();
  const { data: buildInfo } = useBuildInfo();
  const { colorScheme, setColorScheme } = useColorScheme();
  const [settings, setSettings] = useUiSettings();

  // Merged effective config per category.
  const general = useConfigCategory("fhir");
  const logging = useConfigCategory("logging");
  const search = useConfigCategory("search");
  const terminology = useConfigCategory("terminology");
  const packages = useConfigCategory("packages");
  const cache = useConfigCategory("cache");
  const otel = useConfigCategory("otel");
  const dbConsole = useConfigCategory("db_console");
  const serverCfg = useConfigCategory("server");
  const storageCfg = useConfigCategory("storage");
  const redisCfg = useConfigCategory("redis");
  const validation = useConfigCategory("validation");

  const features = useFeatureFlags();
  const toggleFeature = useToggleFeature();
  const setConfig = useSetConfigValue();
  const reload = useReloadConfig();

  const {
    config: formatterConfig,
    isLoading: formatterLoading,
    saveConfig: saveFormatterConfig,
  } = useFormatterSettings();

  const adminAvailable = logging.data !== null || search.data !== null || terminology.data !== null;

  const notifyOpts = (label: string) => ({
    onSuccess: () =>
      notifications.show({ title: "Saved", message: `${label} updated`, color: "green" }),
    onError: (err: unknown) =>
      notifications.show({
        title: "Save failed",
        message: err instanceof Error ? err.message : `Could not update ${label}`,
        color: "red",
      }),
  });

  const save = (category: ConfigCategory, key: string, value: unknown, label: string) =>
    setConfig.mutate({ category, key, value }, notifyOpts(label));

  // Database connection lives under storage.postgres — shown read-only (editing the
  // connection that stores this very config could not be saved or applied safely).
  const pg = subObject(storageCfg.data, "postgres");

  // Active group, gated by admin availability.
  const [active, setActive] = useState("general");
  useEffect(() => {
    if (!adminAvailable && !["appearance", "console"].includes(active)) {
      setActive("appearance");
    }
  }, [adminAvailable, active]);

  const visibleSections = useMemo(
    () =>
      GROUP_SECTIONS.map((sec) => ({
        ...sec,
        items: sec.items.filter((it) => !it.admin || adminAvailable),
      })).filter((sec) => sec.items.length > 0),
    [adminAvailable]
  );

  const activeDef = GROUP_SECTIONS.flatMap((s) => s.items).find((i) => i.id === active);

  const statusColor =
    health?.status === "ok"
      ? "var(--octo-accent-primary)"
      : health?.status === "degraded"
        ? "var(--octo-accent-warm)"
        : "var(--octo-accent-fire)";
  const enabledFeatures = serverSettings
    ? Object.values(serverSettings.features).filter(Boolean).length
    : null;

  return (
    <div className={`${classes.root} page-enter`}>
      {/* ── Hero ── */}
      <header className={classes.hero}>
        <div className={classes.heroInner}>
          <div className={classes.heroTop}>
            <div className={classes.heroTitleWrap}>
              <span className={classes.heroIcon}>
                <SettingsIcon size={22} />
              </span>
              <div>
                <div className={classes.eyebrow}>System</div>
                <h1 className={classes.title}>Settings</h1>
                <p className={classes.description}>
                  Runtime server configuration, feature flags and local preferences.
                </p>
              </div>
            </div>
            <div className={classes.heroActions}>
              <Button
                size="sm"
                variant="subtle"
                onClick={() => refetchHealth()}
                loading={isRefetching}
              >
                <Button.Icon>
                  <Activity size={15} />
                </Button.Icon>
                Test connection
              </Button>
              {adminAvailable ? (
                <Button
                  size="sm"
                  variant="default"
                  loading={reload.isPending}
                  onClick={() =>
                    reload.mutate(undefined, {
                      onSuccess: () =>
                        notifications.show({
                          title: "Reloaded",
                          message: "Configuration reloaded from all sources",
                          color: "green",
                        }),
                      onError: (err) =>
                        notifications.show({
                          title: "Reload failed",
                          message: err instanceof Error ? err.message : "Could not reload",
                          color: "red",
                        }),
                    })
                  }
                >
                  <Button.Icon>
                    <RefreshCw size={15} />
                  </Button.Icon>
                  Reload config
                </Button>
              ) : null}
            </div>
          </div>

          <div className={classes.statGrid}>
            <div className={`${classes.statCard} ${classes.statCardAccent}`}>
              <span className={classes.statLabel}>Server status</span>
              <span className={`${classes.statValue} ${classes.statValueAccent}`}>
                <span className={classes.statDot} style={{ background: statusColor }} />
                {health?.status ?? "unknown"}
              </span>
            </div>
            <div className={classes.statCard}>
              <span className={classes.statLabel}>FHIR version</span>
              <span className={classes.statValue}>{serverSettings?.fhirVersion ?? "—"}</span>
            </div>
            <div className={classes.statCard}>
              <span className={classes.statLabel}>Server version</span>
              <span className={classes.statValue}>{buildInfo?.serverVersion ?? "—"}</span>
            </div>
            <div className={classes.statCard}>
              <span className={classes.statLabel}>Enabled features</span>
              <span className={classes.statValue}>{enabledFeatures ?? "—"}</span>
            </div>
          </div>
        </div>
      </header>

      {/* ── Body: left rail + content ── */}
      <div className={classes.body}>
        <nav className={classes.rail}>
          {visibleSections.map((sec) => (
            <div key={sec.label}>
              <div className={classes.railGroupLabel}>{sec.label}</div>
              {sec.items.map((item) => {
                const Icon = item.icon;
                return (
                  <button
                    key={item.id}
                    type="button"
                    className={`${classes.railItem} ${active === item.id ? classes.railItemActive : ""}`}
                    onClick={() => setActive(item.id)}
                  >
                    <span className={classes.railIcon}>
                      <Icon size={16} />
                    </span>
                    <span className={classes.railLabel}>{item.label}</span>
                    <span className={`${classes.railDot} ${railDotClass[item.kind]}`} />
                  </button>
                );
              })}
            </div>
          ))}
        </nav>

        <div className={classes.scroll}>
          <div className={classes.sections}>
            <div className={classes.groupHeading}>
              <span className={classes.groupTitle}>{activeDef?.label ?? "Settings"}</span>
              <span className={classes.groupSub}>
                {active === "database"
                  ? "Connection settings — read-only here."
                  : activeDef?.kind === "restart"
                    ? "Bootstrap configuration — restart required to apply."
                    : activeDef?.kind === "local"
                      ? "Preferences saved in this browser."
                      : "Applies live without a server restart."}
              </span>
            </div>

            {active === "database" ? (
              <ReadOnlyBanner>
                The connection to PostgreSQL is where this configuration is stored — editing it here
                could not be saved or applied safely. Change it in <code>octofhir.toml</code> or via{" "}
                <code>OCTOFHIR__STORAGE__POSTGRES__*</code> env vars and restart.
              </ReadOnlyBanner>
            ) : activeDef?.kind === "restart" ? (
              <RestartBanner />
            ) : null}

            {/* ── General ── */}
            {active === "general" ? (
              <>
                <Card
                  icon={<SettingsIcon size={16} />}
                  title="FHIR"
                  description="Active FHIR specification version."
                >
                  <div className={classes.fieldGrid}>
                    <Select
                      label="FHIR version"
                      data={fhirVersionOptions}
                      value={str(general.data, "version", "R4")}
                      onChange={(v) => v && save("fhir", "version", v, "FHIR version")}
                    />
                  </div>
                </Card>
                <Card
                  icon={<ScrollText size={16} />}
                  title="Logging"
                  description="Verbosity and output format."
                >
                  <div className={classes.fieldGrid}>
                    <Select
                      label="Log level"
                      description="Lower is more verbose."
                      data={logLevelOptions}
                      value={str(logging.data, "level", "info")}
                      onChange={(v) => v && save("logging", "level", v, "Log level")}
                    />
                    <Select
                      label="Log format"
                      description="JSON for machines, text for humans."
                      data={logFormatOptions}
                      value={str(logging.data, "format", "json")}
                      onChange={(v) => v && save("logging", "format", v, "Log format")}
                    />
                  </div>
                </Card>
              </>
            ) : null}

            {/* ── Search ── */}
            {active === "search" ? (
              <Card
                icon={<Gauge size={16} />}
                title="Search"
                description="Paging limits, registry cache and debug helpers."
              >
                <div className={classes.fieldGrid}>
                  <NumberSetting
                    label="Default page size"
                    description="Used when _count is omitted."
                    value={num(search.data, "default_count", 10)}
                    min={1}
                    max={1000}
                    onCommit={(n) => save("search", "default_count", n, "Default page size")}
                  />
                  <NumberSetting
                    label="Max page size"
                    description="Upper bound for _count."
                    value={num(search.data, "max_count", 100)}
                    min={1}
                    max={10000}
                    onCommit={(n) => save("search", "max_count", n, "Max page size")}
                  />
                  <NumberSetting
                    label="Registry cache capacity"
                    description="Search-parameter registry entries cached."
                    value={num(search.data, "cache_capacity", 1000)}
                    min={0}
                    max={100000}
                    step={100}
                    onCommit={(n) => save("search", "cache_capacity", n, "Cache capacity")}
                  />
                  <NumberSetting
                    label="Max ValueSet expansion"
                    description="Cap on token :in/:above/:below expansion."
                    value={num(search.data, "max_valueset_expansion", 1000)}
                    min={1}
                    max={1000000}
                    step={100}
                    onCommit={(n) => save("search", "max_valueset_expansion", n, "Max expansion")}
                  />
                </div>
                <div className={classes.switchStack}>
                  <Switch
                    label="Allow _debug=search-plan"
                    description="Expose the generated SQL plan."
                    checked={bool(search.data, "allow_debug_search_plan")}
                    onChange={(c) =>
                      save("search", "allow_debug_search_plan", c, "Debug search plan")
                    }
                  />
                  <Switch
                    label="Allow _debug=search-explain-analyze"
                    description="Run EXPLAIN ANALYZE on search queries."
                    checked={bool(search.data, "allow_debug_search_explain_analyze")}
                    onChange={(c) =>
                      save(
                        "search",
                        "allow_debug_search_explain_analyze",
                        c,
                        "Debug explain-analyze"
                      )
                    }
                  />
                </div>
              </Card>
            ) : null}

            {/* ── Terminology ── */}
            {active === "terminology" ? (
              <Card
                icon={<Languages size={16} />}
                title="Terminology"
                description="External code system / ValueSet service."
              >
                <div className={classes.switchStack}>
                  <Switch
                    label="Enable terminology service"
                    checked={bool(terminology.data, "enabled", true)}
                    onChange={(c) => save("terminology", "enabled", c, "Terminology service")}
                  />
                </div>
                <div className={classes.fieldGrid}>
                  <TextSetting
                    label="Server URL"
                    description="FHIR terminology endpoint."
                    value={str(terminology.data, "server_url")}
                    placeholder="https://tx.fhir.org/r4"
                    onCommit={(v) => save("terminology", "server_url", v, "Terminology URL")}
                  />
                  <NumberSetting
                    label="Cache TTL (seconds)"
                    value={num(terminology.data, "cache_ttl_secs", 3600)}
                    min={0}
                    max={86400}
                    step={60}
                    onCommit={(n) =>
                      save("terminology", "cache_ttl_secs", n, "Terminology cache TTL")
                    }
                  />
                </div>
              </Card>
            ) : null}

            {/* ── Packages ── */}
            {active === "packages" ? (
              <Card
                icon={<Boxes size={16} />}
                title="FHIR Packages"
                description="Implementation guides. Saving rebuilds the canonical registry live."
              >
                <PackagesSetting
                  value={
                    Array.isArray(packages.data?.load) ? (packages.data?.load as string[]) : []
                  }
                  onCommit={(list) => save("packages", "load", list, "Packages")}
                />
                <div className={classes.fieldGrid}>
                  <TextSetting
                    label="Package cache directory"
                    description="Optional. Defaults to ~/.fcm/packages."
                    value={str(packages.data, "path")}
                    placeholder="(default)"
                    onCommit={(v) => save("packages", "path", v, "Package path")}
                  />
                </div>
              </Card>
            ) : null}

            {/* ── Cache ── */}
            {active === "cache" ? (
              <Card
                icon={<Layers size={16} />}
                title="Cache"
                description="Two-tier resource and terminology cache."
              >
                <div className={classes.fieldGrid}>
                  <NumberSetting
                    label="Resource cache TTL (s)"
                    description="0 disables the read cache."
                    value={num(cache.data, "resource_ttl_secs", 60)}
                    min={0}
                    max={86400}
                    onCommit={(n) => save("cache", "resource_ttl_secs", n, "Resource TTL")}
                  />
                  <NumberSetting
                    label="Terminology cache TTL (s)"
                    value={num(cache.data, "terminology_ttl_secs", 3600)}
                    min={0}
                    max={86400}
                    step={60}
                    onCommit={(n) => save("cache", "terminology_ttl_secs", n, "Terminology TTL")}
                  />
                  <NumberSetting
                    label="L1 max entries"
                    description="Local DashMap cache capacity."
                    value={num(cache.data, "local_cache_max_entries", 10000)}
                    min={0}
                    max={10000000}
                    step={1000}
                    onCommit={(n) => save("cache", "local_cache_max_entries", n, "L1 cache size")}
                  />
                </div>
              </Card>
            ) : null}

            {/* ── Observability ── */}
            {active === "observability" ? (
              <Card
                icon={<Globe size={16} />}
                title="OpenTelemetry"
                description="Distributed tracing export."
              >
                <div className={classes.switchStack}>
                  <Switch
                    label="Enable OTEL export"
                    checked={bool(otel.data, "enabled")}
                    onChange={(c) => save("otel", "enabled", c, "OTEL export")}
                  />
                </div>
                <div className={classes.fieldGrid}>
                  <TextSetting
                    label="Collector endpoint"
                    value={str(otel.data, "endpoint")}
                    placeholder="http://localhost:4317"
                    onCommit={(v) => save("otel", "endpoint", v, "OTEL endpoint")}
                  />
                  <NumberSetting
                    label="Sample ratio"
                    description="Fraction of traces sampled (0–1)."
                    value={num(otel.data, "sample_ratio", 1)}
                    min={0}
                    max={1}
                    step={0.05}
                    onCommit={(n) => save("otel", "sample_ratio", n, "OTEL sample ratio")}
                  />
                  <TextSetting
                    label="Environment label"
                    description="e.g. dev / staging / prod."
                    value={str(otel.data, "environment")}
                    placeholder="prod"
                    onCommit={(v) => save("otel", "environment", v, "OTEL environment")}
                  />
                </div>
              </Card>
            ) : null}

            {/* ── DB Console ── */}
            {active === "dbconsole" ? (
              <>
                <Card
                  icon={<SlidersHorizontal size={16} />}
                  title="DB Console"
                  description="SQL execution and language-server features."
                >
                  <div className={classes.switchStack}>
                    <Switch
                      label="Enable DB console"
                      checked={bool(dbConsole.data, "enabled", true)}
                      onChange={(c) => save("db_console", "enabled", c, "DB console")}
                    />
                    <Switch
                      label="Enable LSP features"
                      description="Autocomplete, hover, diagnostics."
                      checked={bool(dbConsole.data, "lsp_enabled", true)}
                      onChange={(c) => save("db_console", "lsp_enabled", c, "LSP")}
                    />
                  </div>
                  <div className={classes.fieldGrid}>
                    <Select
                      label="SQL execution mode"
                      data={sqlModeOptions}
                      value={str(dbConsole.data, "sql_mode", "readonly")}
                      onChange={(v) => v && save("db_console", "sql_mode", v, "SQL mode")}
                    />
                    <TextSetting
                      label="Required role"
                      description="Empty = any authenticated user."
                      value={str(dbConsole.data, "required_role")}
                      placeholder="(any)"
                      onCommit={(v) => save("db_console", "required_role", v, "Required role")}
                    />
                  </div>
                </Card>
                <Card
                  icon={<SlidersHorizontal size={16} />}
                  title="SQL Formatter"
                  description="Formatting rules for the SQL editor."
                >
                  {formatterLoading ? (
                    <div className={classes.loadingState}>
                      <Loader size="sm" />
                      <Text variant="body-2" color="secondary">
                        Loading formatter settings…
                      </Text>
                    </div>
                  ) : (
                    <FormatterSettings value={formatterConfig} onChange={saveFormatterConfig} />
                  )}
                </Card>
              </>
            ) : null}

            {/* ── Feature flags ── */}
            {active === "features" ? (
              <Card
                icon={<ToggleRight size={16} />}
                title="Feature flags"
                description="Toggle features at runtime."
              >
                {features.data && features.data.length > 0 ? (
                  <div className={classes.featureList}>
                    {features.data.map((flag) => (
                      <div key={flag.name} className={classes.featureRow}>
                        <div className={classes.featureMeta}>
                          <span className={classes.featureName}>{flag.name}</span>
                          {flag.description ? (
                            <span className={classes.featureDesc}>{flag.description}</span>
                          ) : null}
                        </div>
                        <Switch
                          checked={flag.enabled}
                          onChange={(c) =>
                            toggleFeature.mutate(
                              { name: flag.name, enabled: c },
                              {
                                onError: (err) =>
                                  notifications.show({
                                    title: "Toggle failed",
                                    message: err instanceof Error ? err.message : flag.name,
                                    color: "red",
                                  }),
                              }
                            )
                          }
                        />
                      </div>
                    ))}
                  </div>
                ) : (
                  <span className={classes.hint}>No feature flags registered.</span>
                )}
              </Card>
            ) : null}

            {/* ── Database (read-only) ── */}
            {active === "database" ? (
              <Card
                icon={<Database size={16} />}
                title="PostgreSQL"
                description="Current primary connection and pool settings (read-only)."
              >
                <div className={classes.infoGrid}>
                  <InfoRow label="Host" value={str(pg, "host", "localhost")} />
                  <InfoRow label="Port" value={num(pg, "port", 5432)} />
                  <InfoRow label="Database" value={str(pg, "database", "octofhir")} />
                  <InfoRow label="User" value={str(pg, "user", "postgres")} />
                  <InfoRow label="Pool size" value={num(pg, "pool_size", 10)} />
                  <InfoRow
                    label="Connect timeout"
                    value={`${num(pg, "connect_timeout_ms", 5000)} ms`}
                  />
                  <InfoRow
                    label="Idle timeout"
                    value={`${num(pg, "idle_timeout_ms", 300000)} ms`}
                  />
                  <InfoRow
                    label="Read replica"
                    value={subObject(pg, "read_replica").url ? "configured" : "none"}
                    muted={!subObject(pg, "read_replica").url}
                  />
                </div>
              </Card>
            ) : null}

            {/* ── Redis (restart) ── */}
            {active === "redis" ? (
              <Card
                icon={<HardDrive size={16} />}
                title="Redis"
                description="Optional L2 cache and pub/sub for horizontal scaling."
              >
                <div className={classes.switchStack}>
                  <Switch
                    label="Enable Redis"
                    description="Gracefully degrades when disabled."
                    checked={bool(redisCfg.data, "enabled")}
                    onChange={(c) => save("redis", "enabled", c, "Redis")}
                  />
                </div>
                <div className={classes.fieldGrid}>
                  <TextSetting
                    label="Connection URL"
                    value={str(redisCfg.data, "url", "redis://localhost:6379")}
                    placeholder="redis://localhost:6379"
                    onCommit={(v) => save("redis", "url", v, "Redis URL")}
                  />
                  <NumberSetting
                    label="Pool size"
                    value={num(redisCfg.data, "pool_size", 10)}
                    min={1}
                    max={1000}
                    onCommit={(n) => save("redis", "pool_size", n, "Redis pool size")}
                  />
                  <NumberSetting
                    label="Timeout (ms)"
                    value={num(redisCfg.data, "timeout_ms", 5000)}
                    min={100}
                    max={120000}
                    step={100}
                    onCommit={(n) => save("redis", "timeout_ms", n, "Redis timeout")}
                  />
                </div>
              </Card>
            ) : null}

            {/* ── Server (restart) ── */}
            {active === "server" ? (
              <Card
                icon={<Server size={16} />}
                title="HTTP Server"
                description="Bind address, timeouts and limits."
              >
                <div className={classes.fieldGrid}>
                  <TextSetting
                    label="Bind host"
                    value={str(serverCfg.data, "host", "0.0.0.0")}
                    onCommit={(v) => save("server", "host", v, "Bind host")}
                  />
                  <NumberSetting
                    label="Port"
                    value={num(serverCfg.data, "port", 8080)}
                    min={1}
                    max={65535}
                    onCommit={(n) => save("server", "port", n, "Port")}
                  />
                  <TextSetting
                    label="Base URL"
                    description="Used in links. Empty = derive from host:port."
                    value={str(serverCfg.data, "base_url")}
                    placeholder="(derived)"
                    onCommit={(v) => save("server", "base_url", v, "Base URL")}
                  />
                  <NumberSetting
                    label="Read timeout (ms)"
                    value={num(serverCfg.data, "read_timeout_ms", 15000)}
                    min={1000}
                    max={600000}
                    step={1000}
                    onCommit={(n) => save("server", "read_timeout_ms", n, "Read timeout")}
                  />
                  <NumberSetting
                    label="Write timeout (ms)"
                    value={num(serverCfg.data, "write_timeout_ms", 15000)}
                    min={1000}
                    max={600000}
                    step={1000}
                    onCommit={(n) => save("server", "write_timeout_ms", n, "Write timeout")}
                  />
                  <NumberSetting
                    label="Body limit (bytes)"
                    value={num(serverCfg.data, "body_limit_bytes", 1048576)}
                    min={1024}
                    max={1073741824}
                    step={1024}
                    onCommit={(n) => save("server", "body_limit_bytes", n, "Body limit")}
                  />
                </div>
                <div className={classes.switchStack}>
                  <Switch
                    label="Response compression"
                    checked={bool(serverCfg.data, "compression")}
                    onChange={(c) => save("server", "compression", c, "Compression")}
                  />
                </div>
              </Card>
            ) : null}

            {/* ── Validation (restart) ── */}
            {active === "validation" ? (
              <Card
                icon={<ShieldCheck size={16} />}
                title="Validation"
                description="Resource validation behavior."
              >
                <div className={classes.switchStack}>
                  <Switch
                    label="Allow X-Skip-Validation header"
                    description="Lets clients bypass validation. Security-sensitive."
                    checked={bool(validation.data, "allow_skip_validation")}
                    onChange={(c) =>
                      save("validation", "allow_skip_validation", c, "Skip validation")
                    }
                  />
                  <Switch
                    label="Skip reference validation"
                    description="Disable type + existence checks on references."
                    checked={bool(validation.data, "skip_reference_validation")}
                    onChange={(c) =>
                      save("validation", "skip_reference_validation", c, "Reference validation")
                    }
                  />
                </div>
              </Card>
            ) : null}

            {/* ── Appearance (local) ── */}
            {active === "appearance" ? (
              <Card
                icon={<Palette size={16} />}
                title="Appearance"
                description="Display preferences saved in this browser."
              >
                <div className={classes.fieldGrid}>
                  <Select
                    label="Theme"
                    description="Preferred color scheme."
                    data={themeOptions}
                    value={colorScheme}
                    onChange={(v) => isThemeValue(v) && setColorScheme(v)}
                  />
                </div>
              </Card>
            ) : null}

            {/* ── Console & UI (local) ── */}
            {active === "console" ? (
              <Card
                icon={<Monitor size={16} />}
                title="Console & UI"
                description="Interactive-tool preferences for this browser."
              >
                <div className={classes.fieldGrid}>
                  <NumberSetting
                    label="Request timeout (ms)"
                    description="Console request abort threshold."
                    value={settings.requestTimeoutMs}
                    min={1000}
                    max={120000}
                    step={1000}
                    onCommit={(n) =>
                      setSettings((cur) => ({ ...cur, requestTimeoutMs: n || 30000 }))
                    }
                  />
                </div>
                <div className={classes.switchStack}>
                  <Switch
                    label="Skip request validation"
                    description="Allow malformed paths or missing parameters."
                    checked={settings.skipConsoleValidation}
                    onChange={(c) => setSettings((cur) => ({ ...cur, skipConsoleValidation: c }))}
                  />
                  <Switch
                    label="Allow anonymous REST console requests"
                    description="Send requests without cookies/credentials."
                    checked={settings.allowAnonymousConsoleRequests}
                    onChange={(c) =>
                      setSettings((cur) => ({ ...cur, allowAnonymousConsoleRequests: c }))
                    }
                  />
                  <Switch
                    label="Disable auto-logout on 401/403"
                    description="Keep UI state when the session expires."
                    checked={settings.disableAuthAutoLogout}
                    onChange={(c) => setSettings((cur) => ({ ...cur, disableAuthAutoLogout: c }))}
                  />
                </div>
              </Card>
            ) : null}

            {!adminAvailable ? (
              <Badge color="warm" variant="light">
                Sign in as an administrator to edit server configuration.
              </Badge>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}

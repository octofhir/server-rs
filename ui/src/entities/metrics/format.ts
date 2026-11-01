/**
 * Value formatting by Grafana unit id, so a panel reads the same here and in Grafana.
 * Unknown units fall back to plain numbers.
 */

export const UNITS = [
  { value: "short", label: "Number" },
  { value: "percentunit", label: "Percent (0–1)" },
  { value: "percent", label: "Percent (0–100)" },
  { value: "s", label: "Seconds" },
  { value: "ms", label: "Milliseconds" },
  { value: "reqps", label: "Requests/s" },
  { value: "ops", label: "Operations/s" },
  { value: "bytes", label: "Bytes" },
  { value: "none", label: "None" },
] as const;

function trim(value: number, decimals: number): string {
  return value.toFixed(decimals).replace(/\.0+$|(\.\d*?)0+$/, "$1");
}

function auto(value: number, decimals?: number): string {
  const abs = Math.abs(value);
  if (decimals != null) return value.toFixed(decimals);
  if (abs === 0) return "0";
  if (abs >= 100) return Math.round(value).toLocaleString();
  if (abs >= 10) return trim(value, 1);
  if (abs >= 1) return trim(value, 2);
  return trim(value, 3);
}

function short(value: number, decimals?: number): string {
  const abs = Math.abs(value);
  if (abs >= 1e9) return `${auto(value / 1e9, decimals)} B`;
  if (abs >= 1e6) return `${auto(value / 1e6, decimals)} M`;
  if (abs >= 1e4) return `${auto(value / 1e3, decimals)} K`;
  return auto(value, decimals);
}

function seconds(value: number, decimals?: number): string {
  const abs = Math.abs(value);
  if (abs === 0) return "0 s";
  if (abs < 1e-3) return `${auto(value * 1e6, decimals)} µs`;
  if (abs < 1) return `${auto(value * 1e3, decimals)} ms`;
  if (abs < 60) return `${auto(value, decimals)} s`;
  if (abs < 3600) return `${auto(value / 60, decimals)} min`;
  return `${auto(value / 3600, decimals)} h`;
}

function bytes(value: number, decimals?: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let scaled = value;
  let index = 0;
  while (Math.abs(scaled) >= 1024 && index < units.length - 1) {
    scaled /= 1024;
    index += 1;
  }
  return `${auto(scaled, decimals)} ${units[index]}`;
}

export function formatValue(value: number | null | undefined, unit?: string, decimals?: number): string {
  if (value == null || !Number.isFinite(value)) return "—";
  switch (unit) {
    case "percentunit":
      return `${auto(value * 100, decimals ?? (Math.abs(value) < 0.1 ? 2 : 1))}%`;
    case "percent":
      return `${auto(value, decimals)}%`;
    case "s":
      return seconds(value, decimals);
    case "ms":
      return seconds(value / 1000, decimals);
    case "reqps":
      return `${short(value, decimals)} req/s`;
    case "ops":
      return `${short(value, decimals)} ops/s`;
    case "bytes":
      return bytes(value, decimals);
    case "none":
      return auto(value, decimals);
    default:
      return short(value, decimals);
  }
}

/** Shorter form for axis ticks. */
export function formatAxis(value: number, unit?: string): string {
  if (value === 0) return "0";
  return formatValue(value, unit).replace(" req/s", "").replace(" ops/s", "");
}

export function formatMs(ms: number): string {
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)} s`;
  if (ms >= 100) return `${Math.round(ms)} ms`;
  if (ms >= 10) return `${ms.toFixed(1)} ms`;
  return `${ms.toFixed(2)} ms`;
}

/** Axis ticks are round numbers; skip the decimals the tooltip format adds. */
export function formatAxisMs(ms: number): string {
  if (ms === 0) return "0";
  return ms >= 1000 ? `${ms / 1000} s` : `${ms} ms`;
}

export function formatRate(rate: number): string {
  if (rate >= 100) return Math.round(rate).toLocaleString();
  if (rate >= 10) return rate.toFixed(1);
  return rate.toFixed(2).replace(/\.?0+$/, "") || "0";
}

export function formatShare(share: number): string {
  if (share === 0) return "0%";
  if (share < 0.001) return "<0.1%";
  return `${(share * 100).toFixed(share < 0.1 ? 1 : 0)}%`;
}

// Smoke test for the Prometheus-backed metrics API: status, queries, completions, parse
// diagnostics, built-in dashboards, Grafana export and the Dashboard resource.
const BASE_URL = process.env.BASE_URL ?? "http://localhost:8888";

const tokenRes = await fetch(`${BASE_URL}/auth/token`, {
  method: "POST",
  headers: { "Content-Type": "application/x-www-form-urlencoded" },
  body: new URLSearchParams({
    grant_type: "password",
    username: "admin",
    password: "admin123",
    client_id: "octofhir-ui",
    scope: "user/*.cruds",
  }).toString(),
});
const { access_token } = await tokenRes.json();
const auth = { Authorization: `Bearer ${access_token}`, "Content-Type": "application/json" };

async function call(method: string, path: string, body?: unknown, extra: Record<string, string> = {}) {
  const res = await fetch(`${BASE_URL}${path}`, {
    method,
    headers: { ...auth, ...extra },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let data: unknown = text;
  try {
    data = JSON.parse(text);
  } catch {}
  return { status: res.status, data: data as any };
}

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown) {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok || detail === undefined ? "" : ` → ${JSON.stringify(detail).slice(0, 300)}`}`);
  if (!ok) failures++;
}

// Some traffic so the counters move.
await Promise.all(
  Array.from({ length: 40 }, (_, i) => fetch(`${BASE_URL}/fhir/Patient?_count=1&i=${i}`, { headers: auth }))
);

const settings = await call("GET", "/api/settings");
check("settings advertise metrics", settings.data?.features?.metrics === true, settings.data);

const status = await call("GET", "/api/metrics/status");
check("status reachable", status.data?.reachable === true, status.data);
check("prometheus scrapes this server", (status.data?.targets ?? []).length > 0, status.data);

const end = Math.floor(Date.now() / 1000);
const range = await call("POST", "/api/metrics/query_range", {
  query: "sum(rate(octofhir_http_requests_total[$__rate_interval]))",
  start: end - 600,
  end,
  step: 1,
});
check("query_range ok", range.status === 200 && range.data?.data?.resultType === "matrix", range);
check("step raised to scrape interval", range.data?.window?.step >= 5, range.data?.window);

const scoped = await call("POST", "/api/metrics/query", { query: 'up{job="prometheus"}' });
check("other jobs are invisible", scoped.status === 200 && scoped.data?.data?.result?.length === 0, scoped.data);

const bad = await call("POST", "/api/metrics/query", { query: "sum(" });
check("bad query → 400 bad_query", bad.status === 400 && bad.data?.error?.kind === "bad_query", bad);

const parse = await call("POST", "/api/metrics/parse", { query: "rate(x[$__rate_interval]) +" });
check("parse reports a position as written", parse.data?.ok === false && parse.data?.error?.column != null, parse.data);
const parseOk = await call("POST", "/api/metrics/parse", { query: 'up{instance=~"$instance"}' });
check("parse accepts variables", parseOk.data?.ok === true, parseOk.data);

const metrics = await call("GET", "/api/metrics/metrics");
const http = (metrics.data ?? []).find((m: any) => m.name === "octofhir_http_requests_total");
check("metric catalog has help", http?.help != null, http ?? metrics.data);

const labels = await call("GET", "/api/metrics/labels?metric=octofhir_http_requests_total");
check("labels of a metric", (labels.data ?? []).includes("status_class"), labels.data);
const values = await call("GET", "/api/metrics/label/status_class/values?metric=octofhir_http_requests_total");
check("label values", Array.isArray(values.data) && values.data.length > 0, values.data);

const builtin = await call("GET", "/api/dashboards/builtin");
check("built-in System dashboard", builtin.data?.[0]?.id === "system", builtin.data);

const grafana = await call("POST", "/api/dashboards/grafana/export", builtin.data?.[0]);
check("grafana export", grafana.status === 200 && grafana.data?.__inputs?.[0]?.name === "DS_PROMETHEUS", grafana.data);

const explained = await call("POST", "/api/metrics/explain", {
  query: "sum by (status_class) (rate(octofhir_http_requests_total[$__rate_interval]))",
});
check(
  "explain evaluates each step",
  explained.data?.kind === "aggregation" && explained.data?.children?.[0]?.label === "rate" && explained.data?.result?.series > 0,
  explained.data
);

const reimported = await call("POST", "/api/dashboards/import", grafana.data);
check(
  "grafana JSON imports back",
  reimported.data?.source === "grafana" && reimported.data?.dashboard?.panels?.length === builtin.data?.[0]?.panels?.length,
  reimported.data?.warnings ?? reimported
);

const created = await call(
  "POST",
  "/fhir/Dashboard",
  { resourceType: "Dashboard", title: "Smoke", panels: builtin.data?.[0]?.panels?.slice(0, 2) ?? [] },
  { "X-Skip-Validation": "true" }
);
check("Dashboard resource create", created.status === 201, created);
if (created.data?.id) {
  const listed = await call("GET", "/fhir/Dashboard?_sort=title");
  check("Dashboard search", (listed.data?.entry ?? []).length > 0, listed.data);
  const removed = await call("DELETE", `/fhir/Dashboard/${created.data.id}`);
  check("Dashboard delete", removed.status === 204 || removed.status === 200, removed);
}

const unauth = await fetch(`${BASE_URL}/api/metrics/status`);
check("metrics API requires auth", unauth.status === 401 || unauth.status === 403, unauth.status);

console.log(failures === 0 ? "\nall checks passed" : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);

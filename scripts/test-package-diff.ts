// Checks of POST /api/packages/diff (fhir-diff in the console):
//   bun run scripts/test-package-diff.ts            (BASE_URL defaults to :8888)
// Packages that are not installed are read from the package registry, so the first run needs
// network access; nothing is installed by the comparison.

const BASE_URL = process.env.BASE_URL || "http://localhost:8888";

let failures = 0;
function check(ok: unknown, label: string, detail?: unknown) {
  if (ok) {
    console.log(`  ok   ${label}`);
  } else {
    failures++;
    console.log(`  FAIL ${label}`);
    if (detail !== undefined) console.log("       ", typeof detail === "string" ? detail.slice(0, 600) : JSON.stringify(detail).slice(0, 600));
  }
}

async function getToken(): Promise<string> {
  const res = await fetch(`${BASE_URL}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: "grant_type=password&username=admin&password=admin123&client_id=octofhir-ui&scope=openid%20user/*.cruds",
  });
  if (!res.ok) throw new Error(`token: ${res.status} ${await res.text()}`);
  return (await res.json()).access_token;
}

const token = await getToken();
const headers = { Authorization: `Bearer ${token}`, "Content-Type": "application/json" };

async function diff(body: Record<string, unknown>) {
  const res = await fetch(`${BASE_URL}/api/packages/diff`, { method: "POST", headers, body: JSON.stringify(body) });
  const type = res.headers.get("content-type") ?? "";
  const text = await res.text();
  return { status: res.status, type, text, json: type.includes("json") ? JSON.parse(text) : undefined };
}

async function installed(): Promise<Set<string>> {
  const res = await fetch(`${BASE_URL}/api/packages`, { headers });
  const body = (await res.json()) as { packages: { name: string; version: string }[] };
  return new Set(body.packages.map((p) => `${p.name}@${p.version}`));
}

console.log(`Package diff checks against ${BASE_URL}`);
const before = await installed();

console.log("Package registry");
{
  const res = await fetch(`${BASE_URL}/api/packages/search?q=${encodeURIComponent("us core")}`, { headers });
  const body = (await res.json()) as { packages?: { name: string }[] };
  check(res.ok && body.packages?.some((p) => p.name === "hl7.fhir.us.core"), "search words are wildcards (\"us core\")", body);
  const lookup = await fetch(`${BASE_URL}/api/packages/lookup/hl7.fhir.us.core`, { headers });
  const versions = ((await lookup.json()) as { versions?: string[] }).versions ?? [];
  check(lookup.ok && versions.includes("6.1.0") && versions.includes("9.0.0"), "versions are listed", versions);
}

console.log("Registry packages (not installed)");
{
  const started = performance.now();
  const { status, json } = await diff({ old: "hl7.fhir.us.core@6.1.0", new: "hl7.fhir.us.core@9.0.0" });
  const seconds = ((performance.now() - started) / 1000).toFixed(1);
  check(status === 200, `US Core 6.1.0 → 9.0.0 compared (${seconds}s)`, json);
  const profiles = (json?.profiles ?? []) as { name: string; status: string; changes: { severity: string; rule: string; path: string }[] }[];
  const breaking = profiles.flatMap((p) => p.changes).filter((c) => c.severity === "breaking");
  check(profiles.length === 71, `71 profiles (got ${profiles.length})`);
  check(breaking.length === 66, `66 breaking changes, as the CLI reports (got ${breaking.length})`);
  check(
    breaking.some((c) => c.rule === "min-increased" && c.path === "Location.type.coding"),
    "Location.type.coding min-increased",
  );
  const notes = (json?.notes ?? []) as string[];
  check(notes.some((n) => n.includes("read from the package registry")), "notes say the packages were downloaded", notes);
  const after = await installed();
  check([...after].every((id) => before.has(id)) && after.size === before.size, "nothing was installed");
}

console.log("Downloads");
{
  const markdown = await diff({ old: "hl7.fhir.us.core@6.1.0", new: "hl7.fhir.us.core@9.0.0", format: "markdown" });
  check(markdown.status === 200 && markdown.type.startsWith("text/markdown"), "Markdown content type");
  check(markdown.text.startsWith("# fhir-diff: hl7.fhir.us.core@6.1.0 → hl7.fhir.us.core@9.0.0"), "Markdown report");
  const sarif = await diff({ old: "hl7.fhir.us.core@6.1.0", new: "hl7.fhir.us.core@9.0.0", format: "sarif", minSeverity: "breaking" });
  const results = sarif.json?.runs?.[0]?.results ?? [];
  check(sarif.status === 200 && sarif.json?.version === "2.1.0", "SARIF 2.1.0");
  check(results.length === 66 && results.every((r: { level: string }) => r.level === "error"), `breaking only (${results.length})`);
}

console.log("Filters and errors");
{
  const one = await diff({ old: "hl7.fhir.us.core@6.1.0", new: "hl7.fhir.us.core@9.0.0", profiles: ["USCoreLocationProfile"] });
  check(one.status === 200 && one.json.profiles.length === 1 && one.json.profiles[0].status === "changed", "a renamed profile pairs by URL");
  const bad = await diff({ old: "hl7.fhir.us.core", new: "hl7.fhir.us.core@9.0.0" });
  check(bad.status === 400, "an id without a version is a bad request", bad.text);
  const missing = await diff({ old: "example.does.not.exist@1.0.0", new: "hl7.fhir.us.core@9.0.0" });
  check(missing.status === 404, "an unknown package is not found", missing.text);
}

console.log(failures === 0 ? "\nAll checks passed" : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);

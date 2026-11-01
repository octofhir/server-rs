// Generates mixed FHIR traffic for a few minutes so the Performance dashboards have something
// to show: creates, reads, searches, updates, history and some 404s.
// Usage: bun run scripts/metrics-traffic.ts [seconds]
const BASE_URL = process.env.BASE_URL ?? "http://localhost:8888";
const seconds = Number(process.argv[2] ?? 240);

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
const headers = { Authorization: `Bearer ${access_token}`, "Content-Type": "application/fhir+json" };

const ids: string[] = [];
const families = ["Smith", "Ivanov", "Garcia", "Chen", "Müller", "Okafor"];
const deadline = Date.now() + seconds * 1000;

async function step(i: number) {
  const pick = Math.random();
  if (ids.length < 20 || pick < 0.12) {
    const res = await fetch(`${BASE_URL}/fhir/Patient`, {
      method: "POST",
      headers,
      body: JSON.stringify({ resourceType: "Patient", name: [{ family: families[i % families.length], given: [`P${i}`] }], gender: i % 2 ? "male" : "female" }),
    });
    const body = await res.json().catch(() => null);
    if (body?.id) {
      ids.push(body.id);
      await fetch(`${BASE_URL}/fhir/Observation`, {
        method: "POST",
        headers,
        body: JSON.stringify({
          resourceType: "Observation",
          status: "final",
          code: { coding: [{ system: "http://loinc.org", code: "8867-4" }] },
          subject: { reference: `Patient/${body.id}` },
          valueQuantity: { value: 60 + (i % 40), unit: "/min" },
        }),
      });
    }
    return;
  }
  const id = ids[Math.floor(Math.random() * ids.length)];
  if (pick < 0.5) await fetch(`${BASE_URL}/fhir/Patient/${id}`, { headers });
  else if (pick < 0.75) await fetch(`${BASE_URL}/fhir/Patient?family=${families[i % families.length]}&_count=10`, { headers });
  else if (pick < 0.85) await fetch(`${BASE_URL}/fhir/Observation?subject=Patient/${id}`, { headers });
  else if (pick < 0.92) await fetch(`${BASE_URL}/fhir/Patient/${id}/_history`, { headers });
  else if (pick < 0.97)
    await fetch(`${BASE_URL}/fhir/Patient/${id}`, {
      method: "PUT",
      headers,
      body: JSON.stringify({ resourceType: "Patient", id, active: true, name: [{ family: "Updated" }] }),
    });
  else await fetch(`${BASE_URL}/fhir/Patient/does-not-exist-${i}`, { headers });
}

let i = 0;
while (Date.now() < deadline) {
  // Bursty load: 4–24 concurrent requests, then a short pause.
  const burst = 4 + Math.floor(Math.random() * 20);
  await Promise.all(Array.from({ length: burst }, () => step(i++).catch(() => undefined)));
  await Bun.sleep(150 + Math.random() * 400);
}
console.log(`sent ~${i} requests`);

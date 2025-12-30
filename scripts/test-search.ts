const BASE_URL = process.env.BASE_URL || "http://localhost:8888";
const AUTH_USER = process.env.AUTH_USER || "admin";
const AUTH_PASSWORD = process.env.AUTH_PASSWORD || "admin123";
const CLIENT_ID = process.env.CLIENT_ID || "k6-test";
const CLIENT_SECRET = process.env.CLIENT_SECRET || Bun.file(".k6-secret").text();

async function getToken(): Promise<string> {
  const secret = typeof CLIENT_SECRET === "string" ? CLIENT_SECRET : await CLIENT_SECRET;
  const body = new URLSearchParams({
    grant_type: "password",
    username: AUTH_USER,
    password: AUTH_PASSWORD,
    client_id: CLIENT_ID,
    client_secret: secret.trim(),
  });

  const res = await fetch(`${BASE_URL}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });

  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Token request failed: ${res.status} ${text}`);
  }

  const data = await res.json();
  return data.access_token;
}

async function search(token: string, query: string): Promise<{ status: number; body: any }> {
  const res = await fetch(`${BASE_URL}/fhir/${query}`, {
    headers: {
      Accept: "application/fhir+json",
      Authorization: `Bearer ${token}`,
      // Request debug info
      "X-Debug": "true",
    },
  });

  const text = await res.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }

  return { status: res.status, body };
}

async function testSearch(token: string, query: string) {
  const result = await search(token, query);
  const success = result.status === 200;
  const icon = success ? "✓" : "✗";
  console.log(`[${result.status}] ${icon} ${query}`);

  if (!success) {
    if (result.body?.issue) {
      for (const issue of result.body.issue) {
        console.log(`    ${issue.severity}: ${issue.diagnostics || issue.details?.text || JSON.stringify(issue)}`);
      }
    } else if (typeof result.body === "object") {
      console.log(`    ${JSON.stringify(result.body, null, 2).split("\n").join("\n    ")}`);
    } else {
      console.log(`    ${result.body}`);
    }
  }

  return success;
}

async function main() {
  console.log("Getting token...");
  const token = await getToken();
  console.log("Token obtained ✓\n");

  const searches = [
    // Text search
    "Patient?name=John&_count=10",
    "Patient?name=Undefined&_count=10",
    "Patient?name=Some_long_unexisting_string&_count=10",
    "Patient?name:contains=ohn&_count=10",

    // Date search
    "Patient?birthdate=2007-03-07&_count=10",
    "Patient?birthdate=eq2007-03-07&_count=10",
    "Patient?birthdate=ne2007-03-07&_count=10",
    "Patient?birthdate=lt2007-03-07&_count=10",
    "Patient?birthdate=gt2007-03-07&_count=10",
  ];

  console.log("=== Testing Search Queries ===\n");

  let passed = 0;
  let failed = 0;

  for (const query of searches) {
    const success = await testSearch(token, query);
    if (success) passed++;
    else failed++;
  }

  console.log(`\n=== Results: ${passed} passed, ${failed} failed ===`);

  if (failed > 0) {
    process.exit(1);
  }
}

main().catch(console.error);

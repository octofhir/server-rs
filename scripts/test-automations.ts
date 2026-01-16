/**
 * Automation test script.
 *
 * Tests the automation CRUD API and automation execution functionality.
 *
 * Usage:
 *   bun run scripts/test-automations.ts
 *
 * Prerequisites:
 *   - Server running on localhost:8888
 *   - Database migrations applied
 *   - k6-test client configured (run scripts/k6-setup.ts first)
 */

const BASE_URL = process.env.BASE_URL || "http://localhost:8888";
const AUTH_USER = process.env.AUTH_USER || "admin";
const AUTH_PASSWORD = process.env.AUTH_PASSWORD || "admin123";
const CLIENT_ID = process.env.CLIENT_ID || "k6-test";
const CLIENT_SECRET = process.env.CLIENT_SECRET || Bun.file(".k6-secret").text();

// =============================================================================
// Auth helpers
// =============================================================================

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

// =============================================================================
// Automation API helpers
// =============================================================================

interface Automation {
  id: string;
  name: string;
  description?: string;
  source_code: string;
  status: "active" | "inactive" | "error";
  version: number;
  timeout_ms: number;
  created_at: string;
  updated_at: string;
  triggers?: AutomationTrigger[];
}

interface AutomationTrigger {
  id: string;
  automation_id: string;
  trigger_type: "resource_event" | "cron" | "manual";
  resource_type?: string;
  event_types?: string[];
  fhirpath_filter?: string;
  cron_expression?: string;
  created_at: string;
}

interface ExecutionResult {
  executionId: string;
  success: boolean;
  output?: any;
  error?: string;
  durationMs: number;
}

async function apiRequest<T>(
  token: string,
  method: string,
  path: string,
  body?: object
): Promise<{ status: number; data: T | null; error?: string }> {
  const res = await fetch(`${BASE_URL}${path}`, {
    method,
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
    },
    body: body ? JSON.stringify(body) : undefined,
  });

  const text = await res.text();

  if (!res.ok) {
    let error = text;
    try {
      const parsed = JSON.parse(text);
      error = parsed.issue?.[0]?.diagnostics || JSON.stringify(parsed);
    } catch {}
    return { status: res.status, data: null, error };
  }

  if (!text || res.status === 204) {
    return { status: res.status, data: null };
  }

  try {
    return { status: res.status, data: JSON.parse(text) };
  } catch {
    return { status: res.status, data: null, error: `Invalid JSON: ${text}` };
  }
}

async function listAutomations(token: string): Promise<Automation[]> {
  const result = await apiRequest<{ entry: { resource: Automation }[] }>(
    token,
    "GET",
    "/api/automations"
  );
  return result.data?.entry?.map((e) => e.resource) || [];
}

async function createAutomation(
  token: string,
  automation: {
    name: string;
    description?: string;
    source_code: string;
    timeout_ms?: number;
    triggers?: {
      trigger_type: "resource_event" | "cron" | "manual";
      resource_type?: string;
      event_types?: string[];
      fhirpath_filter?: string;
      cron_expression?: string;
    }[];
  }
): Promise<Automation | null> {
  const result = await apiRequest<Automation>(token, "POST", "/api/automations", automation);
  if (result.error) {
    console.error(`  Error creating automation: ${result.error}`);
    return null;
  }
  return result.data;
}

async function getAutomation(token: string, id: string): Promise<Automation | null> {
  const result = await apiRequest<Automation>(token, "GET", `/api/automations/${id}`);
  return result.data;
}

async function updateAutomation(
  token: string,
  id: string,
  update: {
    name?: string;
    description?: string;
    source_code?: string;
    status?: "active" | "inactive" | "error";
    timeout_ms?: number;
  }
): Promise<Automation | null> {
  const result = await apiRequest<Automation>(token, "PUT", `/api/automations/${id}`, update);
  return result.data;
}

async function deleteAutomation(token: string, id: string): Promise<boolean> {
  const result = await apiRequest(token, "DELETE", `/api/automations/${id}`);
  return result.status === 204;
}

async function deployAutomation(token: string, id: string): Promise<Automation | null> {
  const result = await apiRequest<Automation>(token, "POST", `/api/automations/${id}/deploy`);
  return result.data;
}

async function executeAutomation(
  token: string,
  id: string,
  input?: { resource?: object; event_type?: string }
): Promise<ExecutionResult | null> {
  const result = await apiRequest<ExecutionResult>(
    token,
    "POST",
    `/api/automations/${id}/execute`,
    input || {}
  );
  return result.data;
}

async function getAutomationLogs(
  token: string,
  id: string
): Promise<{ entry: { resource: any }[] } | null> {
  const result = await apiRequest<{ entry: { resource: any }[] }>(
    token,
    "GET",
    `/api/automations/${id}/logs`
  );
  return result.data;
}

async function addTrigger(
  token: string,
  automationId: string,
  trigger: {
    trigger_type: "resource_event" | "cron" | "manual";
    resource_type?: string;
    event_types?: string[];
    fhirpath_filter?: string;
    cron_expression?: string;
  }
): Promise<AutomationTrigger | null> {
  const result = await apiRequest<AutomationTrigger>(
    token,
    "POST",
    `/api/automations/${automationId}/triggers`,
    trigger
  );
  return result.data;
}

async function deleteTrigger(
  token: string,
  automationId: string,
  triggerId: string
): Promise<boolean> {
  const result = await apiRequest(
    token,
    "DELETE",
    `/api/automations/${automationId}/triggers/${triggerId}`
  );
  return result.status === 204;
}

// =============================================================================
// FHIR API helpers
// =============================================================================

async function createResource(
  token: string,
  resource: object
): Promise<object | null> {
  const resourceType = (resource as any).resourceType;
  const res = await fetch(`${BASE_URL}/fhir/${resourceType}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/fhir+json",
      Authorization: `Bearer ${token}`,
    },
    body: JSON.stringify(resource),
  });

  if (!res.ok) {
    const text = await res.text();
    console.error(`  Error creating ${resourceType}: ${res.status} ${text}`);
    return null;
  }

  return res.json();
}

async function deleteResource(
  token: string,
  resourceType: string,
  id: string
): Promise<boolean> {
  const res = await fetch(`${BASE_URL}/fhir/${resourceType}/${id}`, {
    method: "DELETE",
    headers: { Authorization: `Bearer ${token}` },
  });
  return res.status === 200 || res.status === 204;
}

// =============================================================================
// Test functions
// =============================================================================

async function testAutomationCRUD(token: string) {
  console.log("\n=== Test: Automation CRUD Operations ===\n");

  // Create a simple automation
  console.log("1. Creating automation...");
  const automation = await createAutomation(token, {
    name: "Test Automation",
    description: "A simple test automation",
    source_code: `
      // Simple automation that returns the event
      console.log("Automation executed!");
      console.log("Event type:", event.type);
      return { received: event };
    `,
    timeout_ms: 5000,
  });

  if (!automation) {
    console.error("   FAILED: Could not create automation");
    return null;
  }
  console.log(`   Created automation: ${automation.id} (${automation.name})`);
  console.log(`   Status: ${automation.status}, Version: ${automation.version}`);

  // Read the automation
  console.log("\n2. Reading automation...");
  const readAutomation = await getAutomation(token, automation.id);
  if (!readAutomation) {
    console.error("   FAILED: Could not read automation");
    return null;
  }
  console.log(`   Read automation: ${readAutomation.name}`);

  // Update the automation
  console.log("\n3. Updating automation...");
  const updatedAutomation = await updateAutomation(token, automation.id, {
    description: "Updated description",
  });
  if (!updatedAutomation) {
    console.error("   FAILED: Could not update automation");
    return null;
  }
  console.log(`   Updated description: ${updatedAutomation.description}`);
  console.log(`   New version: ${updatedAutomation.version}`);

  // List automations
  console.log("\n4. Listing automations...");
  const automations = await listAutomations(token);
  console.log(`   Found ${automations.length} automation(s)`);

  return automation;
}

async function testAutomationExecution(token: string, automationId: string) {
  console.log("\n=== Test: Automation Execution ===\n");

  // Execute the automation manually
  console.log("1. Executing automation manually...");
  const result = await executeAutomation(token, automationId, {
    event_type: "test",
    resource: { resourceType: "Patient", id: "test-123" },
  });

  if (!result) {
    console.error("   FAILED: Could not execute automation");
    return;
  }

  console.log(`   Execution ID: ${result.executionId}`);
  console.log(`   Success: ${result.success}`);
  console.log(`   Duration: ${result.durationMs}ms`);
  if (result.output) {
    console.log(`   Output: ${JSON.stringify(result.output)}`);
  }
  if (result.error) {
    console.error(`   Error: ${result.error}`);
  }

  // Get execution logs
  console.log("\n2. Getting execution logs...");
  const logs = await getAutomationLogs(token, automationId);
  if (logs?.entry) {
    console.log(`   Found ${logs.entry.length} execution(s)`);
    for (const entry of logs.entry.slice(0, 3)) {
      const exec = entry.resource;
      console.log(
        `   - ${exec.id}: ${exec.status} (${exec.duration_ms || "?"}ms)`
      );
    }
  }
}

async function testAutomationTriggers(token: string, automationId: string) {
  console.log("\n=== Test: Automation Triggers ===\n");

  // Add a resource event trigger
  console.log("1. Adding resource event trigger...");
  const trigger = await addTrigger(token, automationId, {
    trigger_type: "resource_event",
    resource_type: "Patient",
    event_types: ["created", "updated"],
  });

  if (!trigger) {
    console.error("   FAILED: Could not add trigger");
    return;
  }
  console.log(`   Added trigger: ${trigger.id}`);
  console.log(`   Type: ${trigger.trigger_type}`);
  console.log(`   Resource: ${trigger.resource_type}`);
  console.log(`   Events: ${trigger.event_types?.join(", ")}`);

  // Get automation with triggers
  console.log("\n2. Getting automation with triggers...");
  const automation = await getAutomation(token, automationId);
  if (automation?.triggers) {
    console.log(`   Automation has ${automation.triggers.length} trigger(s)`);
  }

  // Deploy the automation
  console.log("\n3. Deploying automation...");
  const deployed = await deployAutomation(token, automationId);
  if (!deployed) {
    console.error("   FAILED: Could not deploy automation");
    return;
  }
  console.log(`   Automation status: ${deployed.status}`);

  // Delete the trigger
  console.log("\n4. Deleting trigger...");
  const deleted = await deleteTrigger(token, automationId, trigger.id);
  console.log(`   Trigger deleted: ${deleted}`);
}

async function testAutomationWithFHIR(token: string) {
  console.log("\n=== Test: Automation with FHIR Operations ===\n");

  // Create an automation that creates a Task when triggered
  console.log("1. Creating FHIR-aware automation...");
  const automation = await createAutomation(token, {
    name: "Welcome Task Automation",
    description: "Creates a welcome task for new patients",
    source_code: `
      // Automation that creates a welcome Task for new patients
      if (event.type === 'created' && event.resource.resourceType === 'Patient') {
        const patient = event.resource;
        const patientName = patient.name?.[0]?.text || patient.name?.[0]?.family || 'Unknown';

        console.log('Creating welcome task for patient:', patientName);

        const task = fhir.create({
          resourceType: 'Task',
          status: 'requested',
          intent: 'order',
          description: 'Welcome patient: ' + patientName,
          for: { reference: 'Patient/' + patient.id }
        });

        console.log('Created task:', task.id);
        return { taskId: task.id };
      }

      return { skipped: true, reason: 'Not a patient creation event' };
    `,
    triggers: [
      {
        trigger_type: "resource_event",
        resource_type: "Patient",
        event_types: ["created"],
      },
    ],
  });

  if (!automation) {
    console.error("   FAILED: Could not create automation");
    return;
  }
  console.log(`   Created automation: ${automation.id}`);

  // Deploy the automation
  console.log("\n2. Deploying automation...");
  const deployed = await deployAutomation(token, automation.id);
  if (!deployed || deployed.status !== "active") {
    console.error("   FAILED: Could not deploy automation");
    await deleteAutomation(token, automation.id);
    return;
  }
  console.log(`   Automation deployed: ${deployed.status}`);

  // Execute manually with a Patient resource
  console.log("\n3. Executing automation with Patient resource...");
  const result = await executeAutomation(token, automation.id, {
    event_type: "created",
    resource: {
      resourceType: "Patient",
      id: "test-patient-001",
      name: [{ family: "Smith", given: ["John"] }],
    },
  });

  if (result) {
    console.log(`   Success: ${result.success}`);
    console.log(`   Duration: ${result.durationMs}ms`);
    if (result.output) {
      console.log(`   Output: ${JSON.stringify(result.output)}`);
    }
    if (result.error) {
      console.error(`   Error: ${result.error}`);
    }
  }

  // Cleanup
  console.log("\n4. Cleaning up...");
  await deleteAutomation(token, automation.id);
  console.log("   Automation deleted");
}

async function testAutomationWithHTTP(token: string) {
  console.log("\n=== Test: Automation with HTTP Requests ===\n");

  // Create an automation that makes an HTTP request using built-in fetch
  console.log("1. Creating HTTP automation...");
  const automation = await createAutomation(token, {
    name: "HTTP Test Automation",
    description: "Tests async fetch functionality",
    source_code: `
      // Automation that fetches data from an API using async fetch
      console.log('Fetching from httpbin...');

      try {
        const response = await fetch('https://httpbin.org/json');

        console.log('Response status:', response.status);
        console.log('Response ok:', response.ok);

        if (response.ok) {
          // Use the standard Response.json() method
          const data = await response.json();
          return {
            success: true,
            status: response.status,
            slideshow: data.slideshow?.title
          };
        } else {
          return { success: false, status: response.status };
        }
      } catch (err) {
        console.error('HTTP error:', err);
        return { success: false, error: String(err) };
      }
    `,
  });

  if (!automation) {
    console.error("   FAILED: Could not create automation");
    return;
  }
  console.log(`   Created automation: ${automation.id}`);

  // Execute the automation
  console.log("\n2. Executing HTTP automation...");
  const result = await executeAutomation(token, automation.id);

  if (result) {
    console.log(`   Automation success: ${result.success}`);
    console.log(`   Duration: ${result.durationMs}ms`);
    if (result.output) {
      console.log(`   Output: ${JSON.stringify(result.output, null, 2)}`);
    }
    if (result.error) {
      console.error(`   Error: ${result.error}`);
    }
  }

  // Cleanup
  console.log("\n3. Cleaning up...");
  await deleteAutomation(token, automation.id);
  console.log("   Automation deleted");
}

async function cleanup(token: string, automationId: string | null) {
  console.log("\n=== Cleanup ===\n");

  if (automationId) {
    console.log(`Deleting automation ${automationId}...`);
    const deleted = await deleteAutomation(token, automationId);
    console.log(`   Deleted: ${deleted}`);
  }
}

// =============================================================================
// Main
// =============================================================================

async function main() {
  console.log("Automation Tests");
  console.log("================\n");
  console.log(`Server: ${BASE_URL}`);

  let token: string;
  try {
    token = await getToken();
    console.log("Authenticated successfully\n");
  } catch (e) {
    console.error(`Authentication failed: ${e}`);
    process.exit(1);
  }

  let testAutomationId: string | null = null;

  try {
    // Test 1: CRUD operations
    const automation = await testAutomationCRUD(token);
    testAutomationId = automation?.id || null;

    if (testAutomationId) {
      // Test 2: Execution
      await testAutomationExecution(token, testAutomationId);

      // Test 3: Triggers
      await testAutomationTriggers(token, testAutomationId);
    }

    // Test 4: Automation with FHIR operations
    await testAutomationWithFHIR(token);

    // Test 5: Automation with HTTP requests
    await testAutomationWithHTTP(token);

    console.log("\n=== All tests completed ===\n");
  } catch (e) {
    console.error(`\nTest error: ${e}`);
  } finally {
    // Cleanup
    await cleanup(token, testAutomationId);
  }
}

main().catch(console.error);

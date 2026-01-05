const BASE_URL = process.env.BASE_URL || "http://localhost:8888";
const AUTH_USER = process.env.AUTH_USER || "admin";
const AUTH_PASSWORD = process.env.AUTH_PASSWORD || "admin123";
const UI_CLIENT_ID = "octofhir-ui";
const K6_CLIENT_ID = "k6-test";

async function getAdminToken(): Promise<string> {
	console.log("Getting admin token...");
	const body = new URLSearchParams({
		grant_type: "password",
		username: AUTH_USER,
		password: AUTH_PASSWORD,
		client_id: UI_CLIENT_ID,
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
	console.log("Admin token obtained ✓");
	return data.access_token;
}

async function checkClientExists(token: string): Promise<boolean> {
	console.log(`\nChecking if ${K6_CLIENT_ID} client exists...`);
	// Note: clientId search parameter may not be indexed, so fetch all and filter
	const res = await fetch(`${BASE_URL}/Client?_count=100`, {
		headers: { Authorization: `Bearer ${token}` },
	});

	if (!res.ok) {
		console.error(`Failed to check client: ${res.status}`);
		return false;
	}

	const data = await res.json();
	const entries = data.entry || [];
	const exists = entries.some(
		(e: { resource?: { clientId?: string } }) =>
			e.resource?.clientId === K6_CLIENT_ID,
	);
	console.log(
		exists
			? `Client ${K6_CLIENT_ID} exists ✓`
			: `Client ${K6_CLIENT_ID} not found`,
	);
	return exists;
}

async function createClient(token: string): Promise<void> {
	console.log(`\nCreating ${K6_CLIENT_ID} client...`);
	const client = {
		resourceType: "Client",
		clientId: K6_CLIENT_ID,
		name: "K6 Load Testing Client",
		confidential: true,
		active: true,
		grantTypes: ["password", "client_credentials"],
		scopes: ["openid", "user/*.cruds", "system/*.cruds"],
	};

	const res = await fetch(`${BASE_URL}/Client`, {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
			Authorization: `Bearer ${token}`,
		},
		body: JSON.stringify(client),
	});

	if (!res.ok) {
		const text = await res.text();
		console.error(`Failed to create client: ${res.status}`);
		try {
			console.error(JSON.stringify(JSON.parse(text), null, 2));
		} catch {
			console.error(text);
		}
		throw new Error("Client creation failed");
	}

	console.log(`Client ${K6_CLIENT_ID} created ✓`);
}

async function regenerateSecret(token: string): Promise<string> {
	console.log("\nRegenerating client secret...");
	const res = await fetch(
		`${BASE_URL}/admin/clients/${K6_CLIENT_ID}/regenerate-secret`,
		{
			method: "POST",
			headers: {
				"Content-Type": "application/json",
				Authorization: `Bearer ${token}`,
			},
		},
	);

	if (!res.ok) {
		const text = await res.text();
		console.error(`Failed to regenerate secret: ${res.status}`);
		console.error(text);
		throw new Error("Secret regeneration failed");
	}

	const data = await res.json();
	console.log("Client secret regenerated ✓");
	return data.clientSecret;
}

async function createAccessPolicy(token: string): Promise<void> {
	console.log("\nCreating/updating AccessPolicy for k6-test...");
	const policy = {
		resourceType: "AccessPolicy",
		id: "k6-test",
		name: "K6 Test Full Access",
		description: "Allow all operations for admin users via k6-test client",
		active: true,
		priority: 1,
		matcher: {
			clients: [K6_CLIENT_ID],
			roles: ["admin"],
		},
		engine: {
			type: "allow",
		},
	};

	const res = await fetch(`${BASE_URL}/AccessPolicy/k6-test`, {
		method: "PUT",
		headers: {
			"Content-Type": "application/json",
			Authorization: `Bearer ${token}`,
		},
		body: JSON.stringify(policy),
	});

	const text = await res.text();
	if (!res.ok) {
		console.error(`Failed to create AccessPolicy: ${res.status}`);
		try {
			console.error(JSON.stringify(JSON.parse(text), null, 2));
		} catch {
			console.error(text);
		}
		throw new Error("AccessPolicy creation failed");
	}

	console.log("AccessPolicy created/updated ✓");
}

async function main() {
	console.log("=== K6 Setup ===\n");

	// Step 1: Get admin token
	const token = await getAdminToken();

	// Step 2: Check if client exists
	const exists = await checkClientExists(token);

	// Step 3: Create client if not exists
	if (!exists) {
		await createClient(token);
	}

	// Step 4: Regenerate secret
	const secret = await regenerateSecret(token);

	// Step 5: Save secret to file
	await Bun.write(".k6-secret", secret);
	console.log("\nClient secret saved to .k6-secret ✓");

	// Step 6: Create AccessPolicy
	await createAccessPolicy(token);

	console.log("\n=== Setup Complete ===");
	console.log("You can now run: just k6-crud-test");
}

main().catch((err) => {
	console.error("\n❌ Setup failed:", err.message);
	process.exit(1);
});

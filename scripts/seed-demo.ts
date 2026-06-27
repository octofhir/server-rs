// Seed a varied demo dataset into the FHIR server for manual console testing.
//   bun run scripts/seed-demo.ts
// Creates Organizations, Practitioners, Patients, Encounters, Conditions and
// Observations with realistic, varied field values.

const BASE_URL = process.env.BASE_URL || "http://localhost:8888";
const CLIENT_ID = process.env.CLIENT_ID || "backend";
const CLIENT_SECRET = process.env.CLIENT_SECRET || "dev-secret-2024";

async function getToken(): Promise<string> {
	const res = await fetch(`${BASE_URL}/auth/token`, {
		method: "POST",
		headers: { "Content-Type": "application/x-www-form-urlencoded" },
		body: new URLSearchParams({
			grant_type: "client_credentials",
			client_id: CLIENT_ID,
			client_secret: CLIENT_SECRET,
			scope: "system/*.cruds",
		}).toString(),
	});
	if (!res.ok)
		throw new Error(`token failed: ${res.status} ${await res.text()}`);
	return ((await res.json()) as { access_token: string }).access_token;
}

let token = "";
async function create(resource: Record<string, unknown>): Promise<string> {
	const res = await fetch(`${BASE_URL}/fhir/${resource.resourceType}`, {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
			Authorization: `Bearer ${token}`,
		},
		body: JSON.stringify(resource),
	});
	if (!res.ok)
		throw new Error(
			`${resource.resourceType} failed: ${res.status} ${await res.text()}`,
		);
	return ((await res.json()) as { id: string }).id;
}

const pick = <T>(arr: T[], i: number): T => arr[i % arr.length];

const GIVEN = [
	"John",
	"Jane",
	"Robert",
	"Mary",
	"Michael",
	"Patricia",
	"David",
	"Linda",
	"James",
	"Barbara",
	"William",
	"Elizabeth",
	"Richard",
	"Susan",
	"Joseph",
	"Jessica",
	"Thomas",
	"Sarah",
	"Charles",
	"Karen",
];
const FAMILY = [
	"Smith",
	"Johnson",
	"Williams",
	"Brown",
	"Jones",
	"Garcia",
	"Miller",
	"Davis",
	"Rodriguez",
	"Martinez",
	"Hernandez",
	"Lopez",
	"Wilson",
	"Anderson",
	"Taylor",
	"Thomas",
	"Moore",
	"Jackson",
	"Martin",
	"Lee",
];
const CITIES = [
	"Boston",
	"Austin",
	"Denver",
	"Seattle",
	"Chicago",
	"Portland",
	"Miami",
	"Phoenix",
];
const ORGS = [
	"Mercy General",
	"Lakeside Clinic",
	"Summit Health",
	"Riverside Medical",
	"Pine Valley Hospital",
];

async function main() {
	token = await getToken();
	console.log(`Seeding ${BASE_URL} …`);

	const orgIds: string[] = [];
	for (let i = 0; i < ORGS.length; i++) {
		orgIds.push(
			await create({
				resourceType: "Organization",
				name: ORGS[i],
				active: true,
				address: [{ city: pick(CITIES, i), country: "US" }],
			}),
		);
	}

	const practIds: string[] = [];
	for (let i = 0; i < 8; i++) {
		practIds.push(
			await create({
				resourceType: "Practitioner",
				active: true,
				name: [
					{
						given: [pick(GIVEN, i + 3)],
						family: pick(FAMILY, i + 5),
						prefix: ["Dr"],
					},
				],
				gender: i % 2 === 0 ? "female" : "male",
			}),
		);
	}

	const genders = ["male", "female", "other", "unknown"];
	let obs = 0;
	let cond = 0;
	let enc = 0;

	for (let i = 0; i < 30; i++) {
		const year = 1945 + ((i * 7) % 70);
		const month = String(1 + (i % 12)).padStart(2, "0");
		const day = String(1 + (i % 27)).padStart(2, "0");
		const patientId = await create({
			resourceType: "Patient",
			active: i % 9 !== 0,
			name: [{ given: [pick(GIVEN, i)], family: pick(FAMILY, i * 3) }],
			gender: pick(genders, i),
			birthDate: `${year}-${month}-${day}`,
			telecom: [
				{
					system: "phone",
					value: `555-01${String(i).padStart(2, "0")}`,
					use: "mobile",
				},
			],
			address: [{ city: pick(CITIES, i), state: "NA", country: "US" }],
			managingOrganization: { reference: `Organization/${pick(orgIds, i)}` },
		});

		// One encounter
		enc++;
		const encId = await create({
			resourceType: "Encounter",
			status: pick(["finished", "in-progress", "planned"], i),
			class: {
				system: "http://terminology.hl7.org/CodeSystem/v3-ActCode",
				code: pick(["AMB", "IMP", "EMER"], i),
				display: "ambulatory",
			},
			subject: { reference: `Patient/${patientId}` },
			participant: [
				{ individual: { reference: `Practitioner/${pick(practIds, i)}` } },
			],
		});

		// A couple of observations (vitals)
		for (let v = 0; v < 2; v++) {
			obs++;
			const isBp = v === 0;
			await create({
				resourceType: "Observation",
				status: pick(["final", "preliminary", "amended"], i + v),
				category: [
					{
						coding: [
							{
								system:
									"http://terminology.hl7.org/CodeSystem/observation-category",
								code: "vital-signs",
								display: "Vital Signs",
							},
						],
					},
				],
				code: isBp
					? {
							coding: [
								{
									system: "http://loinc.org",
									code: "8867-4",
									display: "Heart rate",
								},
							],
							text: "Heart rate",
						}
					: {
							coding: [
								{
									system: "http://loinc.org",
									code: "29463-7",
									display: "Body weight",
								},
							],
							text: "Body weight",
						},
				subject: { reference: `Patient/${patientId}` },
				encounter: { reference: `Encounter/${encId}` },
				effectiveDateTime: `2026-0${1 + (i % 6)}-1${i % 9}T09:00:00Z`,
				valueQuantity: isBp
					? {
							value: 60 + (i % 40),
							unit: "beats/minute",
							system: "http://unitsofmeasure.org",
							code: "/min",
						}
					: {
							value: 50 + (i % 60),
							unit: "kg",
							system: "http://unitsofmeasure.org",
							code: "kg",
						},
			});
		}

		// Every 3rd patient gets a condition
		if (i % 3 === 0) {
			cond++;
			await create({
				resourceType: "Condition",
				clinicalStatus: {
					coding: [
						{
							system:
								"http://terminology.hl7.org/CodeSystem/condition-clinical",
							code: pick(["active", "resolved", "recurrence"], i),
						},
					],
				},
				code: {
					coding: [
						{
							system: "http://snomed.info/sct",
							code: "44054006",
							display: "Diabetes mellitus type 2",
						},
					],
					text: "Type 2 Diabetes",
				},
				subject: { reference: `Patient/${patientId}` },
				recordedDate: `2025-1${i % 3}-15`,
			});
		}
	}

	console.log(
		`Done: ${orgIds.length} Organizations, ${practIds.length} Practitioners, 30 Patients, ${enc} Encounters, ${obs} Observations, ${cond} Conditions.`,
	);
}

main().catch((e) => {
	console.error(e);
	process.exit(1);
});

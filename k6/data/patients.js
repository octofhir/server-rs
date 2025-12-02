// Valid and invalid Patient resources for k6 testing

// Valid Patient resources
export const validPatients = [
  {
    resourceType: "Patient",
    active: true,
    name: [
      {
        use: "official",
        family: "Smith",
        given: ["John", "Jacob"],
      },
    ],
    gender: "male",
    birthDate: "1990-01-15",
    telecom: [
      {
        system: "phone",
        value: "+1-555-123-4567",
        use: "home",
      },
      {
        system: "email",
        value: "john.smith@example.com",
        use: "work",
      },
    ],
    address: [
      {
        use: "home",
        type: "physical",
        line: ["123 Main Street", "Apt 4B"],
        city: "Boston",
        state: "MA",
        postalCode: "02101",
        country: "USA",
      },
    ],
    maritalStatus: {
      coding: [
        {
          system: "http://terminology.hl7.org/CodeSystem/v3-MaritalStatus",
          code: "M",
          display: "Married",
        },
      ],
    },
  },
  {
    resourceType: "Patient",
    active: true,
    name: [
      {
        use: "official",
        family: "Johnson",
        given: ["Emily"],
      },
    ],
    gender: "female",
    birthDate: "1985-06-22",
    telecom: [
      {
        system: "phone",
        value: "+1-555-987-6543",
        use: "mobile",
      },
    ],
    address: [
      {
        use: "home",
        line: ["456 Oak Avenue"],
        city: "Chicago",
        state: "IL",
        postalCode: "60601",
        country: "USA",
      },
    ],
  },
  {
    resourceType: "Patient",
    active: false,
    name: [
      {
        use: "official",
        family: "Williams",
        given: ["Robert", "James"],
      },
      {
        use: "nickname",
        given: ["Bob"],
      },
    ],
    gender: "male",
    birthDate: "1970-12-01",
    deceasedBoolean: false,
    multipleBirthBoolean: true,
  },
  {
    resourceType: "Patient",
    name: [
      {
        use: "official",
        family: "Garcia",
        given: ["Maria", "Elena"],
      },
    ],
    gender: "female",
    birthDate: "1995-03-10",
    communication: [
      {
        language: {
          coding: [
            {
              system: "urn:ietf:bcp:47",
              code: "es",
              display: "Spanish",
            },
          ],
        },
        preferred: true,
      },
      {
        language: {
          coding: [
            {
              system: "urn:ietf:bcp:47",
              code: "en",
              display: "English",
            },
          ],
        },
        preferred: false,
      },
    ],
  },
  {
    resourceType: "Patient",
    identifier: [
      {
        system: "http://hospital.example.org/patient-ids",
        value: "PAT-12345",
      },
      {
        system: "http://hl7.org/fhir/sid/us-ssn",
        value: "123-45-6789",
      },
    ],
    name: [
      {
        use: "official",
        family: "Brown",
        given: ["Michael"],
        prefix: ["Mr."],
        suffix: ["Jr."],
      },
    ],
    gender: "male",
    birthDate: "1988-09-25",
    contact: [
      {
        relationship: [
          {
            coding: [
              {
                system:
                  "http://terminology.hl7.org/CodeSystem/v2-0131",
                code: "N",
                display: "Next-of-Kin",
              },
            ],
          },
        ],
        name: {
          family: "Brown",
          given: ["Sarah"],
        },
        telecom: [
          {
            system: "phone",
            value: "+1-555-111-2222",
          },
        ],
      },
    ],
  },
];

// Minimal valid Patient (with name to pass validation)
export const minimalPatient = {
  resourceType: "Patient",
  name: [
    {
      family: "Minimal",
      given: ["Test"],
    },
  ],
};

// Patient with all optional fields populated
export const fullPatient = {
  resourceType: "Patient",
  identifier: [
    {
      use: "official",
      type: {
        coding: [
          {
            system: "http://terminology.hl7.org/CodeSystem/v2-0203",
            code: "MR",
            display: "Medical Record Number",
          },
        ],
      },
      system: "http://hospital.example.org/mrn",
      value: "MRN-999888777",
      period: {
        start: "2020-01-01",
      },
      assigner: {
        display: "Example Hospital",
      },
    },
  ],
  active: true,
  name: [
    {
      use: "official",
      text: "Dr. Alexandra Thompson III",
      family: "Thompson",
      given: ["Alexandra", "Marie"],
      prefix: ["Dr."],
      suffix: ["III"],
      period: {
        start: "1980-01-01",
      },
    },
  ],
  telecom: [
    {
      system: "phone",
      value: "+1-555-999-8888",
      use: "home",
      rank: 1,
    },
    {
      system: "email",
      value: "athompson@example.com",
      use: "work",
      rank: 2,
    },
  ],
  gender: "female",
  birthDate: "1980-05-15",
  deceasedBoolean: false,
  address: [
    {
      use: "home",
      type: "both",
      text: "789 Pine Street, Suite 100, New York, NY 10001, USA",
      line: ["789 Pine Street", "Suite 100"],
      city: "New York",
      district: "Manhattan",
      state: "NY",
      postalCode: "10001",
      country: "USA",
      period: {
        start: "2015-01-01",
      },
    },
  ],
  maritalStatus: {
    coding: [
      {
        system: "http://terminology.hl7.org/CodeSystem/v3-MaritalStatus",
        code: "S",
        display: "Never Married",
      },
    ],
    text: "Single",
  },
  multipleBirthInteger: 2,
  photo: [
    {
      contentType: "image/png",
      data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
    },
  ],
  communication: [
    {
      language: {
        coding: [
          {
            system: "urn:ietf:bcp:47",
            code: "en-US",
            display: "English (United States)",
          },
        ],
        text: "English",
      },
      preferred: true,
    },
  ],
  generalPractitioner: [
    {
      reference: "Practitioner/example",
      display: "Dr. Smith",
    },
  ],
  managingOrganization: {
    reference: "Organization/example",
    display: "Example Hospital",
  },
};

// Invalid Patient resources for negative testing
export const invalidPatients = {
  // Missing resourceType
  missingResourceType: {
    name: [
      {
        family: "Test",
        given: ["Invalid"],
      },
    ],
    gender: "male",
  },

  // Wrong resourceType
  wrongResourceType: {
    resourceType: "Observation",
    name: [
      {
        family: "Test",
        given: ["Wrong"],
      },
    ],
  },

  // Invalid gender code
  invalidGender: {
    resourceType: "Patient",
    name: [
      {
        family: "Test",
      },
    ],
    gender: "invalid-gender",
  },

  // Invalid date format
  invalidBirthDate: {
    resourceType: "Patient",
    name: [
      {
        family: "Test",
      },
    ],
    birthDate: "not-a-date",
  },

  // Invalid telecom system
  invalidTelecomSystem: {
    resourceType: "Patient",
    telecom: [
      {
        system: "invalid-system",
        value: "123456",
      },
    ],
  },

  // Invalid address use
  invalidAddressUse: {
    resourceType: "Patient",
    address: [
      {
        use: "invalid-use",
        city: "Test City",
      },
    ],
  },

  // Empty object
  emptyObject: {},

  // Null value
  nullValue: null,

  // Array instead of object
  arrayInsteadOfObject: [],

  // String instead of object
  stringInsteadOfObject: "not a patient",

  // Invalid nested structure
  invalidNestedStructure: {
    resourceType: "Patient",
    name: "should be an array",
  },

  // Invalid identifier structure
  invalidIdentifier: {
    resourceType: "Patient",
    identifier: {
      system: "http://example.org",
      value: "123",
    },
  },

  // DeceasedBoolean and deceasedDateTime both present (mutually exclusive)
  conflictingDeceased: {
    resourceType: "Patient",
    deceasedBoolean: true,
    deceasedDateTime: "2023-01-01",
  },

  // MultipleBirthBoolean and multipleBirthInteger both present (mutually exclusive)
  conflictingMultipleBirth: {
    resourceType: "Patient",
    multipleBirthBoolean: true,
    multipleBirthInteger: 2,
  },

  // Invalid reference format
  invalidReference: {
    resourceType: "Patient",
    managingOrganization: {
      reference: 12345,
    },
  },

  // Extra unknown fields (may or may not be rejected depending on server config)
  unknownFields: {
    resourceType: "Patient",
    unknownField: "should be rejected",
    anotherUnknown: {
      nested: "value",
    },
  },

  // Invalid contact structure
  invalidContact: {
    resourceType: "Patient",
    contact: [
      {
        name: "should be HumanName object not string",
      },
    ],
  },
};

// Patient updates for testing PUT operations
export const patientUpdates = {
  // Update name
  nameUpdate: (original) => ({
    ...original,
    name: [
      {
        use: "official",
        family: "UpdatedFamily",
        given: ["UpdatedGiven"],
      },
    ],
  }),

  // Add telecom
  addTelecom: (original) => ({
    ...original,
    telecom: [
      ...(original.telecom || []),
      {
        system: "phone",
        value: "+1-555-NEW-PHONE",
        use: "mobile",
      },
    ],
  }),

  // Update address
  addressUpdate: (original) => ({
    ...original,
    address: [
      {
        use: "home",
        line: ["999 New Address Lane"],
        city: "New City",
        state: "NC",
        postalCode: "12345",
        country: "USA",
      },
    ],
  }),

  // Mark as inactive
  deactivate: (original) => ({
    ...original,
    active: false,
  }),

  // Mark as deceased
  markDeceased: (original) => ({
    ...original,
    deceasedDateTime: new Date().toISOString().split("T")[0],
  }),
};

// Generate a random valid Patient
export function generateRandomPatient() {
  const firstNames = [
    "James",
    "Mary",
    "John",
    "Patricia",
    "Robert",
    "Jennifer",
    "Michael",
    "Linda",
    "William",
    "Elizabeth",
  ];
  const lastNames = [
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
  ];
  const genders = ["male", "female", "other", "unknown"];

  const randomYear = 1940 + Math.floor(Math.random() * 60);
  const randomMonth = String(Math.floor(Math.random() * 12) + 1).padStart(2, "0");
  const randomDay = String(Math.floor(Math.random() * 28) + 1).padStart(2, "0");

  return {
    resourceType: "Patient",
    active: Math.random() > 0.1,
    name: [
      {
        use: "official",
        family: lastNames[Math.floor(Math.random() * lastNames.length)],
        given: [firstNames[Math.floor(Math.random() * firstNames.length)]],
      },
    ],
    gender: genders[Math.floor(Math.random() * genders.length)],
    birthDate: `${randomYear}-${randomMonth}-${randomDay}`,
    telecom: [
      {
        system: "phone",
        value: `+1-555-${String(Math.floor(Math.random() * 900) + 100)}-${String(Math.floor(Math.random() * 9000) + 1000)}`,
        use: "mobile",
      },
    ],
  };
}

// Generate batch of random patients
export function generatePatientBatch(count) {
  const patients = [];
  for (let i = 0; i < count; i++) {
    patients.push(generateRandomPatient());
  }
  return patients;
}

import { createSignal, For, Show } from "solid-js";
import styles from "./CapabilityStatementPage.module.css";
import { Card } from "@/shared/ui/Card";

// Mock data for CapabilityStatement
const mockCapabilityStatement = {
    resourceType: "CapabilityStatement",
    status: "active",
    date: "2025-11-24",
    publisher: "OctoFHIR",
    kind: "instance",
    software: {
        name: "OctoFHIR Server",
        version: "0.1.0",
        releaseDate: "2025-11-24"
    },
    implementation: {
        description: "OctoFHIR Server Reference Implementation",
        url: "http://localhost:8080/fhir"
    },
    fhirVersion: "4.0.1",
    format: ["application/fhir+json", "application/fhir+xml"],
    rest: [
        {
            mode: "server",
            resource: [
                {
                    type: "Patient",
                    interaction: [{ code: "read" }, { code: "vread" }, { code: "search-type" }, { code: "update" }, { code: "create" }]
                },
                {
                    type: "Observation",
                    interaction: [{ code: "read" }, { code: "search-type" }, { code: "create" }]
                },
                {
                    type: "Encounter",
                    interaction: [{ code: "read" }, { code: "search-type" }]
                },
                {
                    type: "Practitioner",
                    interaction: [{ code: "read" }, { code: "search-type" }]
                }
            ]
        }
    ]
};

export const CapabilityStatementPage = () => {
    const [capability] = createSignal(mockCapabilityStatement);

    return (
        <div class={styles.container}>
            <div class={styles.header}>
                <h1 class={styles.title}>Capability Statement</h1>
                <p class={styles.subtitle}>Server metadata and supported resources</p>
            </div>

            <div class={styles.section}>
                <h2 class={styles.sectionTitle}>Server Information</h2>
                <Card class={styles.infoGrid} padding="md">
                    <div class={styles.infoItem}>
                        <span class={styles.infoLabel}>Software</span>
                        <span class={styles.infoValue}>{capability().software.name} v{capability().software.version}</span>
                    </div>
                    <div class={styles.infoItem}>
                        <span class={styles.infoLabel}>FHIR Version</span>
                        <span class={styles.infoValue}>{capability().fhirVersion}</span>
                    </div>
                    <div class={styles.infoItem}>
                        <span class={styles.infoLabel}>Publisher</span>
                        <span class={styles.infoValue}>{capability().publisher}</span>
                    </div>
                    <div class={styles.infoItem}>
                        <span class={styles.infoLabel}>Status</span>
                        <span class={styles.infoValue} style={{ "text-transform": "capitalize" }}>{capability().status}</span>
                    </div>
                </Card>
            </div>

            <div class={styles.section}>
                <h2 class={styles.sectionTitle}>Supported Resources</h2>
                <div class={styles.grid}>
                    <For each={capability().rest[0].resource}>
                        {(resource) => (
                            <div class={styles.resourceCard}>
                                <div class={styles.resourceName}>{resource.type}</div>
                                <div class={styles.interactionList}>
                                    <For each={resource.interaction}>
                                        {(interaction) => (
                                            <span class={styles.interactionBadge}>{interaction.code}</span>
                                        )}
                                    </For>
                                </div>
                            </div>
                        )}
                    </For>
                </div>
            </div>
        </div>
    );
};

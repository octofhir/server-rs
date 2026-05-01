import type { ReactNode } from "react";
import type { StatusTone } from "#/shared/ui";

export interface FhirPackageSummary {
    id?: string;
    name: string;
    version: string;
    fhirVersion?: string;
    resourceCount?: number;
    installedAt?: string;
    description?: ReactNode;
    isCompatible?: boolean;
}

export interface FhirPackageRegistryResult {
    name: string;
    latestVersion: string;
    versions: string[];
    description?: ReactNode;
    installedVersions?: string[];
}

export interface FhirPackageResourceTypeSummary {
    resourceType: string;
    count: number;
    tone?: StatusTone;
}

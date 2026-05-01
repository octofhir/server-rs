import type { CategorizedResourceTypesResponse } from "@/shared/api/types";

export interface FhirCatalogSummary {
	total: number;
	fhir: number;
	system: number;
	custom: number;
	caption: string;
}

export function getFhirCatalogSummary(
	catalog: CategorizedResourceTypesResponse | undefined,
): FhirCatalogSummary {
	const counts = catalog?.counts;
	const total = counts?.all ?? catalog?.types.length ?? 0;
	const fhir = counts?.fhir ?? catalog?.types.filter((type) => type.category === "fhir").length ?? 0;
	const system =
		counts?.system ?? catalog?.types.filter((type) => type.category === "system").length ?? 0;
	const custom =
		counts?.custom ?? catalog?.types.filter((type) => type.category === "custom").length ?? 0;

	return {
		total,
		fhir,
		system,
		custom,
		caption: `${fhir} FHIR, ${system} system, ${custom} custom`,
	};
}

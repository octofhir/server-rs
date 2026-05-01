import type { CategorizedResourceType, CategorizedResourceTypesResponse } from "@/shared/api/types";

export type FhirCatalogCategoryFilter = "all" | "fhir" | "system" | "custom";
export type FhirCatalogCategoryTone = "info" | "warning" | "danger";

export interface FhirCatalogCategoryOption {
	value: FhirCatalogCategoryFilter;
	label: string;
}

export interface FhirCatalogTypeView {
	id: string;
	name: string;
	packageName: string;
	definitionUrl?: string;
	category: CategorizedResourceType["category"];
	categoryTone: FhirCatalogCategoryTone;
}

const categoryToneByType: Record<CategorizedResourceType["category"], FhirCatalogCategoryTone> = {
	fhir: "info",
	system: "warning",
	custom: "danger",
};

export function getFhirCatalogCategoryOptions(
	catalog: CategorizedResourceTypesResponse | undefined,
): FhirCatalogCategoryOption[] {
	const counts = catalog?.counts;

	return [
		{ value: "all", label: `All${counts ? ` (${counts.all})` : ""}` },
		{ value: "fhir", label: `FHIR${counts ? ` (${counts.fhir})` : ""}` },
		{ value: "system", label: `System${counts ? ` (${counts.system})` : ""}` },
		{ value: "custom", label: `Custom${counts ? ` (${counts.custom})` : ""}` },
	];
}

export function filterFhirCatalogTypes(
	catalog: CategorizedResourceTypesResponse | undefined,
	categoryFilter: FhirCatalogCategoryFilter,
	search: string,
): CategorizedResourceType[] {
	if (!catalog?.types) return [];

	const query = search.trim().toLowerCase();
	return catalog.types.filter((type) => {
		if (categoryFilter !== "all" && type.category !== categoryFilter) {
			return false;
		}

		return !query || type.name.toLowerCase().includes(query);
	});
}

export function getFhirCatalogTypeViews(
	types: CategorizedResourceType[],
): FhirCatalogTypeView[] {
	return types.map((type) => ({
		id: type.name,
		name: type.name,
		packageName: type.package,
		definitionUrl: type.url,
		category: type.category,
		categoryTone: categoryToneByType[type.category],
	}));
}

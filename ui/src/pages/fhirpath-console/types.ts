export interface FhirPathMetadata {
	evaluator: string;
	expression: string;
	resultCount: number;
	timing: {
		parseTime: number;
		evaluationTime: number;
		totalTime: number;
	};
}

export interface FhirPathResult {
	datatype: string;
	value: unknown; // The actual value (string, number, object, etc.)
	index: number;
}

export interface FhirPathEvaluationResponse {
	metadata: FhirPathMetadata;
	results: FhirPathResult[];
}

function findPart(param: { part?: Array<{ name: string }> }, name: string) {
	return param?.part?.find((p) => p.name === name);
}

function extractValue(param: {
	valueString?: string;
	valueInteger?: number;
	valueBoolean?: boolean;
	valueDecimal?: number;
	resource?: unknown;
	valueHumanName?: unknown;
	valueAddress?: unknown;
	valueIdentifier?: unknown;
	valueCodeableConcept?: unknown;
	valueCoding?: unknown;
	valueReference?: unknown;
	valuePeriod?: unknown;
	valueRange?: unknown;
	valueRatio?: unknown;
	valueContactPoint?: unknown;
	valueCode?: string;
	valueId?: string;
	valueUri?: string;
	valueUrl?: string;
	valueDate?: string;
	valueDateTime?: string;
	valueTime?: string;
	valueQuantity?: unknown;
}): unknown {
	// Extract value from valueString, valueInteger, valueBoolean, etc.
	if (param.valueString !== undefined) return param.valueString;
	if (param.valueInteger !== undefined) return param.valueInteger;
	if (param.valueBoolean !== undefined) return param.valueBoolean;
	if (param.valueDecimal !== undefined) return param.valueDecimal;
	if (param.resource !== undefined) return param.resource;
	if (param.valueHumanName !== undefined) return param.valueHumanName;
	if (param.valueAddress !== undefined) return param.valueAddress;
	if (param.valueIdentifier !== undefined) return param.valueIdentifier;
	if (param.valueCodeableConcept !== undefined)
		return param.valueCodeableConcept;
	if (param.valueCoding !== undefined) return param.valueCoding;
	if (param.valueReference !== undefined) return param.valueReference;
	if (param.valuePeriod !== undefined) return param.valuePeriod;
	if (param.valueRange !== undefined) return param.valueRange;
	if (param.valueRatio !== undefined) return param.valueRatio;
	if (param.valueContactPoint !== undefined) return param.valueContactPoint;
	if (param.valueCode !== undefined) return param.valueCode;
	if (param.valueId !== undefined) return param.valueId;
	if (param.valueUri !== undefined) return param.valueUri;
	if (param.valueUrl !== undefined) return param.valueUrl;
	if (param.valueDate !== undefined) return param.valueDate;
	if (param.valueDateTime !== undefined) return param.valueDateTime;
	if (param.valueTime !== undefined) return param.valueTime;
	if (param.valueQuantity !== undefined) return param.valueQuantity;
	return null;
}

export function parseParametersResponse(
	params: {
		parameter?: Array<{
			name: string;
			part?: Array<{
				name: string;
				valueString?: string;
				valueInteger?: number;
				valueDecimal?: number;
				part?: Array<{
					name: string;
					valueDecimal?: number;
				}>;
			}>;
		}>;
	},
): FhirPathEvaluationResponse {
	const metadataParam = params.parameter?.find((p) => p.name === "metadata");
	const resultParams =
		params.parameter?.filter((p) => p.name !== "metadata") || [];

	const timingPart = findPart(metadataParam, "timing") as {
		part?: Array<{ name: string; valueDecimal?: number }>;
	};

	const metadata: FhirPathMetadata = {
		evaluator: findPart(metadataParam, "evaluator")?.valueString || "",
		expression: findPart(metadataParam, "expression")?.valueString || "",
		resultCount: findPart(metadataParam, "resultCount")?.valueInteger || 0,
		timing: {
			parseTime:
				timingPart?.part?.find((p) => p.name === "parseTime")?.valueDecimal ||
				0,
			evaluationTime:
				timingPart?.part?.find((p) => p.name === "evaluationTime")
					?.valueDecimal || 0,
			totalTime:
				timingPart?.part?.find((p) => p.name === "totalTime")?.valueDecimal ||
				0,
		},
	};

	const results: FhirPathResult[] = resultParams.map((param) => {
		const datatype = param.name;
		const value = extractValue(param);
		const indexPart = findPart(param, "index") as { valueInteger?: number };
		const index = indexPart?.valueInteger || 0;

		return {
			datatype,
			value,
			index,
		};
	});

	return { metadata, results };
}

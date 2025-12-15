export interface ValidationResult {
	isValid: boolean;
	errors: string[];
}

export function validateConsoleRequest(params: {
	method: string;
	path: string;
	body?: string;
	headers?: Record<string, string>;
}): ValidationResult {
	const errors: string[] = [];

	// Validate method
	if (!["GET", "POST", "PUT", "PATCH", "DELETE"].includes(params.method)) {
		errors.push(`Invalid HTTP method: ${params.method}`);
	}

	// Validate path
	if (!params.path || params.path.trim() === "") {
		errors.push("Request path is required");
	}

	if (params.path && !params.path.startsWith("/")) {
		errors.push("Path must start with /");
	}

	// Validate body for POST/PUT/PATCH
	if (["POST", "PUT", "PATCH"].includes(params.method) && params.body) {
		try {
			JSON.parse(params.body);
		} catch (e) {
			const errorMessage = e instanceof Error ? e.message : "Unknown error";
			errors.push(`Invalid JSON body: ${errorMessage}`);
		}
	}

	// Validate headers
	if (params.headers) {
		for (const [key, value] of Object.entries(params.headers)) {
			if (!key || key.trim() === "") {
				errors.push("Header name cannot be empty");
			}
			if (value === undefined || value === null) {
				errors.push(`Header "${key}" has invalid value`);
			}
		}
	}

	return {
		isValid: errors.length === 0,
		errors,
	};
}

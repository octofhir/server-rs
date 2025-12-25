import { useMutation } from "@tanstack/react-query";
import { useUnit } from "effector-react";
import { fhirClient, HttpError } from "@/shared/api/fhirClient";
import { $mode, setLastResponse } from "../state/consoleStore";
import { validateConsoleRequest } from "../utils/requestValidator";
import { notifications } from "@mantine/notifications";
import { useHistory } from "./useHistory";
import type { HttpMethod } from "@/shared/api";
import { useUiSettings } from "@/shared";

export interface SendRequestParams {
	method: HttpMethod;
	path: string;
	body?: string;
	headers?: Record<string, string>;
}

export interface RequestResponse {
	id: string; // UUID
	status: number;
	statusText: string;
	durationMs: number;
	body?: unknown;
	headers?: Record<string, string>;
	requestedAt: string; // ISO timestamp
	requestMethod: HttpMethod;
	requestPath: string;
	requestBody?: string;
	requestHeaders?: Record<string, string>;
}

export function useSendConsoleRequest() {
	const { setLastResponse: setLastResponseEvent, mode } = useUnit({
		setLastResponse,
		mode: $mode,
	});
	const { addEntry } = useHistory();
	const [settings] = useUiSettings();

	return useMutation({
		mutationFn: async (params: SendRequestParams): Promise<RequestResponse> => {
			// 1. Validate request
			if (!settings.skipConsoleValidation) {
				const validation = validateConsoleRequest(params);
				if (!validation.isValid) {
					throw new Error(validation.errors.join(", "));
				}
			}

			// 2. Parse body if present
			let parsedBody: unknown = undefined;
			if (params.body && params.body.trim() !== "") {
				try {
					parsedBody = JSON.parse(params.body);
				} catch (e) {
					const errorMessage = e instanceof Error ? e.message : "Unknown error";
					throw new Error(`Invalid JSON body: ${errorMessage}`);
				}
			}

			// 3. Execute request with timing
			const startTime = performance.now();
			const requestedAt = new Date().toISOString();

			// Convert path to absolute URL to send exactly as user typed
			let absolutePath = params.path;
			if (!absolutePath.startsWith("http://") && !absolutePath.startsWith("https://")) {
				// Ensure path starts with /
				if (!absolutePath.startsWith("/")) {
					absolutePath = `/${absolutePath}`;
				}
				absolutePath = `${window.location.origin}${absolutePath}`;
			}

			try {
				const response = await fhirClient.rawRequest<unknown>(
					params.method,
					absolutePath,
					parsedBody,
					{
						timeout: settings.requestTimeoutMs,
						includeCredentials: !settings.allowAnonymousConsoleRequests,
						headers: params.headers,
					},
				);

				const endTime = performance.now();
				const durationMs = Math.round(endTime - startTime);

				return {
					id: crypto.randomUUID(),
					status: response.status,
					statusText: response.statusText,
					durationMs,
					body: response.data,
					headers: response.headers,
					requestedAt,
					requestMethod: params.method,
					requestPath: params.path,
					requestBody: params.body,
					requestHeaders: params.headers,
				};
			} catch (error: unknown) {
				const endTime = performance.now();
				const durationMs = Math.round(endTime - startTime);

				// Extract status and body from HttpError
				if (error instanceof HttpError) {
					return {
						id: crypto.randomUUID(),
						status: error.response.status,
						statusText: error.response.statusText,
						durationMs,
						body: error.response.data,
						headers: error.response.headers,
						requestedAt,
						requestMethod: params.method,
						requestPath: params.path,
						requestBody: params.body,
						requestHeaders: params.headers,
					};
				}

				// Handle other errors (network, timeout, etc.)
				const errorMessage =
					error instanceof Error ? error.message : "Unknown error";
				return {
					id: crypto.randomUUID(),
					status: 0,
					statusText: errorMessage,
					durationMs,
					body: undefined,
					headers: undefined,
					requestedAt,
					requestMethod: params.method,
					requestPath: params.path,
					requestBody: params.body,
					requestHeaders: params.headers,
				};
			}
		},

		onSuccess: async (data, _variables) => {
			// Update store with last response
			setLastResponseEvent({
				status: data.status,
				statusText: data.statusText,
				durationMs: data.durationMs,
				body: data.body,
				requestedAt: data.requestedAt,
			});

			// Add to IndexedDB history
			try {
				await addEntry({
					method: data.requestMethod,
					path: data.requestPath,
					body: data.requestBody,
					headers: data.requestHeaders,
					requestedAt: data.requestedAt,
					responseStatus: data.status,
					responseStatusText: data.statusText,
					responseDurationMs: data.durationMs,
					responseBody: data.body,
					responseHeaders: data.headers,
					resourceType: extractResourceType(data.requestPath, data.body),
					isPinned: false,
					mode,
				});
				console.log("✅ Added to history:", data.requestPath);
			} catch (error) {
				console.error("❌ Failed to add to history:", error);
				notifications.show({
					title: "History Error",
					message: "Failed to save request to history",
					color: "orange",
				});
			}

			// Show notification
			const isSuccess = data.status >= 200 && data.status < 300;
			notifications.show({
				title: isSuccess ? "Request Successful" : "Request Completed",
				message: `${data.status} ${data.statusText} (${data.durationMs}ms)`,
				color: isSuccess ? "green" : data.status >= 400 ? "red" : "yellow",
			});
		},

		onError: (error: Error) => {
			notifications.show({
				title: "Request Failed",
				message: error.message,
				color: "red",
			});
		},
	});
}

function extractResourceType(path: string, body?: unknown): string | undefined {
	// Extract from path: /fhir/Patient -> Patient
	const pathMatch = path.match(/\/([A-Z][a-zA-Z]+)/);
	if (pathMatch) return pathMatch[1];

	// Extract from body
	if (body && typeof body === "object" && "resourceType" in body) {
		return body.resourceType as string;
	}

	return undefined;
}

import { useState, useEffect, useCallback, useRef } from "react";
import type { LogEntry, LogLevel, LogFilters } from "@/shared/api/types";
import { isRecord } from "@/shared/api/guards";

// Generate a unique ID for log entries
function generateId(): string {
	return `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
}

// Demo log targets for realistic mock data
const DEMO_TARGETS = [
	"octofhir_server::handlers",
	"octofhir_server::middleware",
	"octofhir_server::auth",
	"octofhir_db_postgres::storage",
	"octofhir_search::parser",
	"octofhir_fhirpath::evaluator",
	"octofhir_config::watcher",
	"hyper::server",
	"sqlx::query",
	"tower_http::trace",
];

const DEMO_MESSAGES: Record<LogLevel, string[]> = {
	trace: [
		"Entering function scope",
		"Variable state captured",
		"Iterator step completed",
		"Buffer flushed",
	],
	debug: [
		"Processing request parameters",
		"Cache lookup: key=Patient/123",
		"SQL query prepared",
		"Token validation started",
		"Search parameters parsed",
	],
	info: [
		"Request processed successfully",
		"New connection established",
		"Resource created: Patient/abc123",
		"Search completed: 42 results",
		"Configuration reloaded",
		"Package installed: hl7.fhir.r4.core@4.0.1",
	],
	warn: [
		"Deprecated API usage detected",
		"Rate limit threshold approaching",
		"Cache miss for frequently accessed resource",
		"Slow query detected: 1250ms",
		"Token expiring soon",
	],
	error: [
		"Failed to connect to database",
		"Invalid resource format",
		"Authentication failed: invalid credentials",
		"Search parameter not supported",
		"Transaction rollback triggered",
	],
};

const LOG_LEVELS: readonly string[] = ["trace", "debug", "info", "warn", "error"];

function isLogLevel(value: unknown): value is LogLevel {
	return typeof value === "string" && LOG_LEVELS.includes(value);
}

function isLogEntry(value: unknown): value is LogEntry {
	if (!isRecord(value)) {
		return false;
	}

	const span = value.span;

	return (
		typeof value.id === "string" &&
		typeof value.timestamp === "string" &&
		isLogLevel(value.level) &&
		typeof value.target === "string" &&
		typeof value.message === "string" &&
		(value.fields === undefined || isRecord(value.fields)) &&
		(span === undefined ||
			(isRecord(span) && typeof span.name === "string" && typeof span.target === "string"))
	);
}

// Generate a random demo log entry
function generateDemoLogEntry(): LogEntry {
	const levels: LogLevel[] = ["trace", "debug", "info", "warn", "error"];
	const weights = [0.1, 0.2, 0.5, 0.15, 0.05]; // Weighted distribution

	const random = Math.random();
	let cumulative = 0;
	let level: LogLevel = "info";
	for (let i = 0; i < weights.length; i++) {
		cumulative += weights[i];
		if (random <= cumulative) {
			level = levels[i];
			break;
		}
	}

	const messages = DEMO_MESSAGES[level];
	const message = messages[Math.floor(Math.random() * messages.length)];
	const target = DEMO_TARGETS[Math.floor(Math.random() * DEMO_TARGETS.length)];

	const entry: LogEntry = {
		id: generateId(),
		timestamp: new Date().toISOString(),
		level,
		target,
		message,
	};

	// Add fields for some entries
	if (Math.random() > 0.6) {
		const fieldOptions: Record<string, unknown>[] = [
			{ method: "GET", path: "/Patient", duration_ms: Math.floor(Math.random() * 200) },
			{ resource_type: "Patient", count: Math.floor(Math.random() * 100) },
			{ client_id: "web-console", user_id: "admin" },
			{ query_time_ms: Math.floor(Math.random() * 500), rows_affected: Math.floor(Math.random() * 10) },
		];
		entry.fields = fieldOptions[Math.floor(Math.random() * fieldOptions.length)];
	}

	return entry;
}

interface UseLogStreamOptions {
	maxEntries?: number;
	autoScroll?: boolean;
	demoMode?: boolean;
	demoInterval?: number;
}

interface UseLogStreamReturn {
	logs: LogEntry[];
	isConnected: boolean;
	isPaused: boolean;
	filters: LogFilters;
	connectionError: string | null;
	pause: () => void;
	resume: () => void;
	clear: () => void;
	setFilters: (filters: Partial<LogFilters>) => void;
	exportLogs: (format: "json" | "text") => void;
}

export function useLogStream(options: UseLogStreamOptions = {}): UseLogStreamReturn {
	const {
		maxEntries = 1000,
		demoMode = true, // Default to demo mode until backend is ready
		demoInterval = 1500,
	} = options;

	const [logs, setLogs] = useState<LogEntry[]>([]);
	const [isConnected, setIsConnected] = useState(false);
	const [isPaused, setIsPaused] = useState(false);
	const [connectionError, setConnectionError] = useState<string | null>(null);
	const [filters, setFiltersState] = useState<LogFilters>({
		levels: ["trace", "debug", "info", "warn", "error"],
	});

	const wsRef = useRef<WebSocket | null>(null);
	const demoIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
	const pendingLogsRef = useRef<LogEntry[]>([]);

	// Add a log entry (respecting max entries limit)
	const addLogEntry = useCallback(
		(entry: LogEntry) => {
			if (isPaused) {
				pendingLogsRef.current.push(entry);
				return;
			}

			setLogs((prev) => {
				const newLogs = [...prev, entry];
				if (newLogs.length > maxEntries) {
					return newLogs.slice(-maxEntries);
				}
				return newLogs;
			});
		},
		[isPaused, maxEntries]
	);

	// Filter logs based on current filters
	const filteredLogs = logs.filter((log) => {
		// Level filter
		if (!filters.levels.includes(log.level)) {
			return false;
		}

		// Search filter
		if (filters.search) {
			const searchLower = filters.search.toLowerCase();
			const matchesMessage = log.message.toLowerCase().includes(searchLower);
			const matchesTarget = log.target.toLowerCase().includes(searchLower);
			const matchesFields = log.fields
				? JSON.stringify(log.fields).toLowerCase().includes(searchLower)
				: false;
			if (!matchesMessage && !matchesTarget && !matchesFields) {
				return false;
			}
		}

		// Target filter
		if (filters.target && !log.target.includes(filters.target)) {
			return false;
		}

		// Time range filter
		if (filters.startTime && new Date(log.timestamp) < new Date(filters.startTime)) {
			return false;
		}
		if (filters.endTime && new Date(log.timestamp) > new Date(filters.endTime)) {
			return false;
		}

		return true;
	});

	// Demo mode: generate fake logs
	useEffect(() => {
		if (!demoMode || isPaused) {
			if (demoIntervalRef.current) {
				clearInterval(demoIntervalRef.current);
				demoIntervalRef.current = null;
			}
			return;
		}

		setIsConnected(true);
		setConnectionError(null);

		// Generate initial batch
		const initialLogs: LogEntry[] = [];
		for (let i = 0; i < 20; i++) {
			const entry = generateDemoLogEntry();
			// Backdate timestamps
			entry.timestamp = new Date(Date.now() - (20 - i) * 2000).toISOString();
			initialLogs.push(entry);
		}
		setLogs(initialLogs);

		// Generate new logs at interval
		demoIntervalRef.current = setInterval(() => {
			addLogEntry(generateDemoLogEntry());
		}, demoInterval);

		return () => {
			if (demoIntervalRef.current) {
				clearInterval(demoIntervalRef.current);
			}
		};
	}, [demoMode, demoInterval, isPaused, addLogEntry]);

	// WebSocket mode (for when backend is ready)
	useEffect(() => {
		if (demoMode) return;

		const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
		const wsUrl = `${protocol}//${window.location.host}/api/logs/stream`;

		try {
			const ws = new WebSocket(wsUrl);
			wsRef.current = ws;

			ws.onopen = () => {
				setIsConnected(true);
				setConnectionError(null);
			};

			ws.onmessage = (event) => {
				try {
					const entry = JSON.parse(event.data);
					if (!isLogEntry(entry)) {
						throw new Error("Invalid log entry payload");
					}
					addLogEntry(entry);
				} catch {
					setConnectionError("Received an invalid log entry");
				}
			};

			ws.onerror = () => {
				setConnectionError("WebSocket connection error");
			};

			ws.onclose = () => {
				setIsConnected(false);
			};

			return () => {
				ws.close();
			};
		} catch (error) {
			setConnectionError(error instanceof Error ? error.message : "Failed to connect");
		}
	}, [demoMode, addLogEntry]);

	const pause = useCallback(() => {
		setIsPaused(true);
	}, []);

	const resume = useCallback(() => {
		setIsPaused(false);
		// Add pending logs
		if (pendingLogsRef.current.length > 0) {
			setLogs((prev) => {
				const newLogs = [...prev, ...pendingLogsRef.current];
				pendingLogsRef.current = [];
				if (newLogs.length > maxEntries) {
					return newLogs.slice(-maxEntries);
				}
				return newLogs;
			});
		}
	}, [maxEntries]);

	const clear = useCallback(() => {
		setLogs([]);
		pendingLogsRef.current = [];
	}, []);

	const setFilters = useCallback((newFilters: Partial<LogFilters>) => {
		setFiltersState((prev) => ({ ...prev, ...newFilters }));
	}, []);

	const exportLogs = useCallback(
		(format: "json" | "text") => {
			const logsToExport = filteredLogs;

			let content: string;
			let filename: string;
			let mimeType: string;

			if (format === "json") {
				content = JSON.stringify(logsToExport, null, 2);
				filename = `logs-${new Date().toISOString().slice(0, 19).replace(/:/g, "-")}.json`;
				mimeType = "application/json";
			} else {
				content = logsToExport
					.map((log) => {
						let line = `[${log.timestamp}] [${log.level.toUpperCase().padEnd(5)}] [${log.target}] ${log.message}`;
						if (log.fields) {
							line += ` ${JSON.stringify(log.fields)}`;
						}
						return line;
					})
					.join("\n");
				filename = `logs-${new Date().toISOString().slice(0, 19).replace(/:/g, "-")}.txt`;
				mimeType = "text/plain";
			}

			const blob = new Blob([content], { type: mimeType });
			const url = URL.createObjectURL(blob);
			const a = document.createElement("a");
			a.href = url;
			a.download = filename;
			document.body.appendChild(a);
			a.click();
			document.body.removeChild(a);
			URL.revokeObjectURL(url);
		},
		[filteredLogs]
	);

	return {
		logs: filteredLogs,
		isConnected,
		isPaused,
		filters,
		connectionError,
		pause,
		resume,
		clear,
		setFilters,
		exportLogs,
	};
}

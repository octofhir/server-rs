import type { ConsoleCommand } from "./types";

/**
 * Check if query fuzzy matches target string
 * @param query - Search query
 * @param target - String to match against
 * @returns true if all characters in query appear in order in target
 *
 * @example
 * fuzzyMatch('pt', 'Patient') // true
 * fuzzyMatch('get', 'GET /fhir/Patient') // true
 * fuzzyMatch('xyz', 'Patient') // false
 */
export function fuzzyMatch(query: string, target: string): boolean {
	if (!query) return true;
	if (!target) return false;

	const lowerQuery = query.toLowerCase();
	const lowerTarget = target.toLowerCase();

	let queryIndex = 0;

	for (
		let i = 0;
		i < lowerTarget.length && queryIndex < lowerQuery.length;
		i++
	) {
		if (lowerTarget[i] === lowerQuery[queryIndex]) {
			queryIndex++;
		}
	}

	return queryIndex === lowerQuery.length;
}

/**
 * Score how well query matches target
 * Higher scores indicate better matches
 *
 * @param query - Search query
 * @param target - String to match against
 * @returns Score from 0-100, or 0 if no match
 *
 * Scoring:
 * - 100: Exact match
 * - 90: Starts with query
 * - 80: Contains query as substring
 * - 50: Fuzzy match (characters appear in order)
 * - 0: No match
 */
export function scoreMatch(query: string, target: string): number {
	if (!query) return 100;
	if (!fuzzyMatch(query, target)) return 0;

	const lowerQuery = query.toLowerCase();
	const lowerTarget = target.toLowerCase();

	// Exact match
	if (lowerTarget === lowerQuery) return 100;

	// Starts with query
	if (lowerTarget.startsWith(lowerQuery)) return 90;

	// Contains query as substring
	if (lowerTarget.includes(lowerQuery)) return 80;

	// Fuzzy match (characters in order)
	return 50;
}

/**
 * Filter and sort commands by search query using fuzzy matching
 *
 * Searches across command label, description, and keywords.
 * Returns commands sorted by match quality (best matches first).
 *
 * @param commands - Array of commands to filter
 * @param query - Search query
 * @returns Filtered and sorted array of commands
 *
 * @example
 * const commands = [
 *   { id: '1', label: 'GET /fhir/Patient', category: 'history' },
 *   { id: '2', label: 'POST /fhir/Observation', category: 'history' }
 * ];
 * filterAndSortCommands(commands, 'pat') // Returns Patient command first
 */
export function filterAndSortCommands(
	commands: ConsoleCommand[],
	query: string,
): ConsoleCommand[] {
	if (!query.trim()) {
		return commands;
	}

	// Score each command
	const scored = commands
		.map((cmd) => {
			const labelScore = scoreMatch(query, cmd.label);
			const descriptionScore = cmd.description
				? scoreMatch(query, cmd.description)
				: 0;
			const keywordScores = cmd.keywords?.map((k) => scoreMatch(query, k)) || [];

			// Take the maximum score from all fields
			const maxScore = Math.max(
				labelScore,
				descriptionScore,
				...keywordScores,
			);

			return {
				command: cmd,
				score: maxScore,
			};
		})
		.filter((item) => item.score > 0)
		.sort((a, b) => {
			// Sort by score descending
			if (b.score !== a.score) {
				return b.score - a.score;
			}
			// Secondary sort: alphabetically by label
			return a.command.label.localeCompare(b.command.label);
		});

	return scored.map((item) => item.command);
}

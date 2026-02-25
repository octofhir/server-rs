import type * as Monaco from "monaco-editor";

export const LANGUAGE_ID = "fhir-query";

export function registerFhirQueryLanguage(monaco: typeof Monaco): void {
	if (monaco.languages.getLanguages().some((l) => l.id === LANGUAGE_ID)) {
		return;
	}

	monaco.languages.register({ id: LANGUAGE_ID });

	monaco.languages.setMonarchTokensProvider(LANGUAGE_ID, {
		defaultToken: "string",

		tokenizer: {
			root: [
				// Base paths
				[/\/fhir/, "keyword"],
				[/\/api/, "keyword"],

				// Operations ($validate, $everything, etc.)
				[/\$[\w-]+/, "annotation"],

				// Query string delimiters
				[/[?&]/, "delimiter"],
				[/=/, "delimiter.equals"],
				[/,/, "delimiter.comma"],

				// Modifier (`:exact`, `:contains`, etc.)
				[/:[\w-]+/, "keyword.modifier"],

				// Special params (_count, _sort, _summary, etc.)
				[/_[\w-]+/, "variable.special"],

				// Search prefixes (ge, le, gt, lt, eq, ne, sa, eb, ap)
				[/(?<==[,-]?)(eq|ne|gt|lt|ge|le|sa|eb|ap)(?=\d)/, "keyword.prefix"],

				// Resource types (PascalCase after /)
				[/(?<=\/)[A-Z][a-zA-Z]+/, "type.identifier"],

				// Resource ID (after ResourceType/)
				[/(?<=\/[A-Z][a-zA-Z]+\/)[a-zA-Z0-9._-]+/, "string.id"],

				// Path separators
				[/\//, "delimiter.slash"],

				// Param names (before = or :)
				[/[a-z][\w.-]*(?=[:=])/, "variable"],

				// Values
				[/[^\s?&=,:/]+/, "string.value"],
			],
		},
	} as Monaco.languages.IMonarchLanguage);

	monaco.languages.setLanguageConfiguration(LANGUAGE_ID, {
		brackets: [],
		autoClosingPairs: [],
		surroundingPairs: [],
		wordPattern: /[a-zA-Z0-9_.$-]+/,
	});
}

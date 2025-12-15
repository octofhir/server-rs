export interface ConsoleSearchParamToken {
	id: string;
	code: string;
	modifier?: string;
	value?: string;
	resourceType?: string;
	fromMetadata?: boolean;
}

export function formatSearchParamToken(token: ConsoleSearchParamToken): string {
	const modifier = token.modifier ? `:${token.modifier}` : "";
	const value = token.value ? `=${token.value}` : "";
	return `${token.code}${modifier}${value}`;
}

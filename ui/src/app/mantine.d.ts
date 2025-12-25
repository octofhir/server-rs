import "@mantine/core";
import type { BrandTokens, SemanticTokens } from "./theme";

declare module "@mantine/core" {
	// Extend Mantine theme typing for theme.other
	export interface MantineThemeOther {
		brand: BrandTokens;
		semantic: SemanticTokens;
		headerHeight: number;
		sidebarWidth: number;
		sidebarCollapsedWidth: number;
		contentMaxWidth: number;
	}
}

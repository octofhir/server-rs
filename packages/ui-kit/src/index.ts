// Base Gravity UI stylesheet + OctoFHIR overrides — must load before any
// component import so Gravity's `.g-root` tokens are present in the DOM.
import "./styles";

// App layer
export * from "./app";

// Shared layer — theme
export * from "./shared/theme";

// Shared layer — UI components
export * from "./shared/ui";

// Shared layer — hooks
export * from "./shared/hooks";

// Shared layer — lib/utilities
export * from "./shared/lib";

// Widgets layer — composed product UI patterns
export * from "./widgets";

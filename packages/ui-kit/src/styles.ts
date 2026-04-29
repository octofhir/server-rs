/**
 * Side-effect-only entry point for Gravity UI base styles + OctoFHIR overrides.
 *
 * Importing the package barrel (`@octofhir/ui-kit`) already triggers these via
 * the index re-export chain, but consumers can also import this file
 * explicitly to guarantee CSS load order (e.g. before any Monaco/CodeMirror
 * styles that may set conflicting tokens).
 *
 *     import "@octofhir/ui-kit/styles";
 *
 * Order matters:
 *   1. Gravity fonts (Inter font-face declarations)
 *   2. Gravity base tokens (`.g-root` block, theme variants)
 *   3. OctoFHIR brand fonts
 *   4. OctoFHIR → Gravity variable overrides
 */

import "@gravity-ui/uikit/styles/fonts.css";
import "@gravity-ui/uikit/styles/styles.css";
import "./shared/theme/fonts.css";
import "./shared/theme/gravity-overrides.css";

/**
 * Side-effect-only entry point for the OctoFHIR ui-kit base styles.
 *
 * Importing the package barrel (`@octofhir/ui-kit`) already triggers these via
 * the index re-export chain, but consumers can also import this file
 * explicitly to guarantee CSS load order (e.g. before any Monaco/CodeMirror
 * styles that may set conflicting tokens).
 *
 *     import "@octofhir/ui-kit/styles";
 *
 * Loads the UI typeface (Inter) plus the self-hosted mono font. Design tokens
 * are emitted as `--octo-*` CSS variables by `UIProvider`.
 */

import "@fontsource-variable/inter/wght.css";
import "./shared/theme/fonts.css";

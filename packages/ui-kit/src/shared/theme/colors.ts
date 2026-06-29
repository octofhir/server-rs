/**
 * OctoFHIR brand palette — built on the OKLCH color space for perceptual
 * uniformity. Each scale runs from 50 (lightest) → 950 (darkest) by index
 * 0..9. Index 5 is the "Base" stop used as the canonical brand stop and is
 * what the dark/light schemes pull from.
 *
 * Scale stops feed the `--octo-*` design tokens (see tokens.ts) so that
 * brand-mapped CSS variables (`--octo-accent-primary`, `--octo-text-primary`,
 * etc.) read coherently across the light and dark schemes.
 */

export const palette = {
    /** Mint / Emerald — primary brand accent. Soft, modern, distinctive. */
    primary: [
        "oklch(98% 0.018 168)",
        "oklch(95% 0.040 168)",
        "oklch(90% 0.072 168)",
        "oklch(82% 0.120 168)",
        "oklch(73% 0.155 168)",
        "oklch(64% 0.168 168)", // Base
        "oklch(56% 0.152 168)", // Hover
        "oklch(48% 0.130 168)", // Active
        "oklch(38% 0.100 168)",
        "oklch(28% 0.070 168)",
    ],
    /** Emerald — success. */
    success: [
        "oklch(98% 0.014 150)",
        "oklch(95% 0.038 150)",
        "oklch(89% 0.075 150)",
        "oklch(82% 0.120 150)",
        "oklch(74% 0.150 150)",
        "oklch(64% 0.165 150)", // Base
        "oklch(54% 0.140 150)", // Hover
        "oklch(45% 0.115 150)", // Active
        "oklch(36% 0.090 150)",
        "oklch(27% 0.065 150)",
    ],
    /** Amber — warning. */
    warning: [
        "oklch(98% 0.018 80)",
        "oklch(96% 0.045 80)",
        "oklch(91% 0.090 80)",
        "oklch(86% 0.135 80)",
        "oklch(80% 0.165 80)",
        "oklch(74% 0.180 80)", // Base
        "oklch(64% 0.155 80)", // Hover
        "oklch(54% 0.125 80)", // Active
        "oklch(43% 0.095 80)",
        "oklch(32% 0.065 80)",
    ],
    /** Crimson — destructive / error. */
    error: [
        "oklch(98% 0.014 25)",
        "oklch(95% 0.040 25)",
        "oklch(90% 0.080 25)",
        "oklch(82% 0.130 25)",
        "oklch(73% 0.175 25)",
        "oklch(63% 0.200 25)", // Base
        "oklch(55% 0.180 25)", // Hover
        "oklch(46% 0.150 25)", // Active
        "oklch(37% 0.115 25)",
        "oklch(28% 0.085 25)",
    ],
    /** Iris — informational accent. */
    info: [
        "oklch(98% 0.014 230)",
        "oklch(95% 0.036 230)",
        "oklch(90% 0.072 230)",
        "oklch(82% 0.125 230)",
        "oklch(73% 0.165 230)",
        "oklch(62% 0.188 230)", // Base
        "oklch(54% 0.170 230)", // Hover
        "oklch(46% 0.142 230)", // Active
        "oklch(37% 0.108 230)",
        "oklch(28% 0.080 230)",
    ],
    /** Violet — secondary brand accent for highlights / decorations. */
    accent: [
        "oklch(98% 0.014 295)",
        "oklch(95% 0.040 295)",
        "oklch(90% 0.085 295)",
        "oklch(82% 0.145 295)",
        "oklch(72% 0.190 295)",
        "oklch(62% 0.210 295)", // Base
        "oklch(54% 0.192 295)", // Hover
        "oklch(46% 0.162 295)", // Active
        "oklch(37% 0.125 295)",
        "oklch(27% 0.092 295)",
    ],
    /** Slate / Navy — surface depth, dark-mode backgrounds. */
    deep: [
        "oklch(98% 0.005 258)",
        "oklch(94% 0.010 258)",
        "oklch(88% 0.018 258)",
        "oklch(78% 0.028 258)",
        "oklch(66% 0.038 258)",
        "oklch(54% 0.048 258)", // Base
        "oklch(42% 0.052 258)", // Hover
        "oklch(32% 0.052 258)", // Active
        "oklch(22% 0.046 258)",
        "oklch(14% 0.034 258)",
    ],
    /** Cool neutral gray — surfaces, borders, text. */
    gray: [
        "oklch(99% 0.002 258)",
        "oklch(97% 0.004 258)",
        "oklch(93% 0.007 258)",
        "oklch(86% 0.010 258)",
        "oklch(75% 0.012 258)",
        "oklch(62% 0.014 258)", // Base
        "oklch(50% 0.014 258)", // Hover
        "oklch(38% 0.014 258)", // Active
        "oklch(26% 0.012 258)",
        "oklch(16% 0.010 258)",
    ],
} as const;

export type PaletteHue = keyof typeof palette;
export type PaletteStop = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9;

/**
 * OctoFHIR brand palette (Marigold + graphite) — built on the OKLCH color space for perceptual
 * uniformity. Each scale runs from 50 (lightest) → 950 (darkest) by index
 * 0..9. Index 5 is the "Base" stop used as the canonical brand stop and is
 * what the dark/light schemes pull from.
 *
 * Scale stops feed the `--octo-*` design tokens (see tokens.ts) so that
 * brand-mapped CSS variables (`--octo-accent-primary`, `--octo-text-primary`,
 * etc.) read coherently across the light and dark schemes.
 */

export const palette = {
    /** Marigold — the single brand accent. Fills use the base stop with dark text on top. */
    primary: [
        "oklch(98.5% 0.022 90)",
        "oklch(95.5% 0.055 88)",
        "oklch(91% 0.100 86)",
        "oklch(86% 0.140 84)",
        "oklch(83% 0.155 83)",
        "oklch(80% 0.160 82)", // Base
        "oklch(75% 0.158 78)", // Hover
        "oklch(66% 0.140 74)", // Active
        "oklch(50% 0.110 70)", // Text on light
        "oklch(36% 0.080 70)",
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
    /** Orange — warning. Kept well away from the Marigold accent. */
    warning: [
        "oklch(98% 0.016 55)",
        "oklch(95% 0.040 55)",
        "oklch(90% 0.080 55)",
        "oklch(83% 0.125 55)",
        "oklch(75% 0.160 55)",
        "oklch(66% 0.185 50)", // Base
        "oklch(58% 0.175 46)", // Hover
        "oklch(50% 0.150 44)", // Active
        "oklch(41% 0.120 44)",
        "oklch(32% 0.090 44)",
    ],
    /** Crimson — destructive / error. */
    error: [
        "oklch(98% 0.014 12)",
        "oklch(95% 0.035 12)",
        "oklch(90% 0.075 12)",
        "oklch(82% 0.125 12)",
        "oklch(72% 0.175 12)",
        "oklch(60% 0.215 12)", // Base
        "oklch(52% 0.200 12)", // Hover
        "oklch(45% 0.170 12)", // Active
        "oklch(37% 0.130 12)",
        "oklch(28% 0.095 12)",
    ],
    /** Blue — informational. */
    info: [
        "oklch(98% 0.012 260)",
        "oklch(95% 0.030 260)",
        "oklch(90% 0.060 260)",
        "oklch(82% 0.105 260)",
        "oklch(72% 0.150 260)",
        "oklch(58% 0.200 262)", // Base
        "oklch(50% 0.200 262)", // Hover
        "oklch(43% 0.180 262)", // Active
        "oklch(35% 0.140 262)",
        "oklch(27% 0.100 262)",
    ],
    /** Graphite — secondary accent kept neutral so Marigold stays the only hue. */
    accent: [
        "oklch(98% 0.003 286)",
        "oklch(95% 0.005 286)",
        "oklch(90% 0.008 286)",
        "oklch(82% 0.010 286)",
        "oklch(70% 0.012 286)",
        "oklch(55% 0.014 286)", // Base
        "oklch(47% 0.014 286)", // Hover
        "oklch(40% 0.012 286)", // Active
        "oklch(30% 0.010 286)",
        "oklch(22% 0.008 286)",
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

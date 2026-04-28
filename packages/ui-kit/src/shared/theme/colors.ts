/**
 * OctoFHIR brand palette — built on the OKLCH color space for perceptual
 * uniformity. Each scale runs from 50 (lightest) → 950 (darkest) by index
 * 0..9. Index 5 is the "Base" stop used as the canonical brand stop and is
 * what the dark/light schemes pull from.
 *
 * The palette is tuned for parity with Gravity UI's internal hue ramp so that
 * brand-mapped CSS variables (`--g-color-base-brand`, `--g-color-text-brand`,
 * etc.) read coherently when overridden via `gravity-overrides.css`.
 */

export const palette = {
    /** Indigo / Azure — primary brand accent. */
    primary: [
        "oklch(98% 0.012 258)",
        "oklch(95% 0.030 258)",
        "oklch(90% 0.060 258)",
        "oklch(82% 0.110 258)",
        "oklch(73% 0.155 258)",
        "oklch(62% 0.180 258)", // Base
        "oklch(54% 0.175 258)", // Hover
        "oklch(46% 0.155 258)", // Active
        "oklch(36% 0.115 258)",
        "oklch(26% 0.080 258)",
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
    /** Iris — informational accent (Gravity "info" mapping). */
    info: [
        "oklch(98% 0.012 230)",
        "oklch(95% 0.030 230)",
        "oklch(90% 0.060 230)",
        "oklch(82% 0.110 230)",
        "oklch(73% 0.150 230)",
        "oklch(64% 0.170 230)", // Base
        "oklch(56% 0.155 230)", // Hover
        "oklch(48% 0.130 230)", // Active
        "oklch(38% 0.100 230)",
        "oklch(28% 0.075 230)",
    ],
    /** Violet — secondary brand accent for highlights / decorations. */
    accent: [
        "oklch(98% 0.012 295)",
        "oklch(95% 0.035 295)",
        "oklch(90% 0.075 295)",
        "oklch(82% 0.130 295)",
        "oklch(73% 0.175 295)",
        "oklch(63% 0.195 295)", // Base
        "oklch(55% 0.180 295)", // Hover
        "oklch(47% 0.155 295)", // Active
        "oklch(37% 0.120 295)",
        "oklch(27% 0.090 295)",
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

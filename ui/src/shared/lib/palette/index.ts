export interface ColorPalette {
  primary: string[];
  accent: string;
  background: string;
  surface: string;
}

export interface PaletteGenerationOptions {
  colorCount?: number;
  saturationRange?: [number, number];
  lightnessRange?: [number, number];
}

// Color extraction from canvas
export const extractColorsFromImage = async (
  imagePath: string,
  options: PaletteGenerationOptions = {}
): Promise<ColorPalette> => {
  const { colorCount = 9 } = options;

  return new Promise((resolve, reject) => {
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d");
    const img = new Image();

    if (!ctx) {
      reject(new Error("Could not get canvas context"));
      return;
    }

    img.crossOrigin = "anonymous";
    img.onload = () => {
      try {
        canvas.width = img.width;
        canvas.height = img.height;
        ctx.drawImage(img, 0, 0);

        const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
        const colors = extractDominantColors(imageData.data, colorCount);

        const palette = generatePalette(colors);
        resolve(palette);
      } catch (error) {
        reject(error);
      }
    };

    img.onerror = () => reject(new Error("Failed to load image"));
    img.src = imagePath;
  });
};

const extractDominantColors = (imageData: Uint8ClampedArray, count: number): string[] => {
  const colorMap = new Map<string, number>();

  // Sample every 4th pixel for performance
  for (let i = 0; i < imageData.length; i += 16) {
    const r = imageData[i];
    const g = imageData[i + 1];
    const b = imageData[i + 2];
    const alpha = imageData[i + 3];

    // Skip transparent pixels
    if (alpha < 125) continue;

    // Round to reduce color variations
    const roundedR = Math.round(r / 10) * 10;
    const roundedG = Math.round(g / 10) * 10;
    const roundedB = Math.round(b / 10) * 10;

    const colorKey = `${roundedR},${roundedG},${roundedB}`;
    colorMap.set(colorKey, (colorMap.get(colorKey) || 0) + 1);
  }

  // Sort by frequency and take top colors
  const sortedColors = Array.from(colorMap.entries())
    .sort((a, b) => b[1] - a[1])
    .slice(0, count)
    .map(([color]) => {
      const [r, g, b] = color.split(",").map(Number);
      return rgbToHex(r, g, b);
    });

  return sortedColors;
};

const generatePalette = (dominantColors: string[]): ColorPalette => {
  const primaryColor = dominantColors[0] || "#1976d2";
  const accentColor = dominantColors[1] || "#dc004e";

  // Generate primary color scale (50-900)
  const primary = generateColorScale(primaryColor, 9);

  return {
    primary,
    accent: accentColor,
    background: "#ffffff",
    surface: "#f5f5f5",
  };
};

const generateColorScale = (baseColor: string, steps: number): string[] => {
  const hsl = hexToHsl(baseColor);
  const scale: string[] = [];

  for (let i = 0; i < steps; i++) {
    const lightness = 95 - (i * 85) / (steps - 1); // 95% to 10%
    const scaledColor = hslToHex(hsl.h, hsl.s, lightness);
    scale.push(scaledColor);
  }

  return scale;
};

// Color utility functions
const rgbToHex = (r: number, g: number, b: number): string => {
  return `#${((1 << 24) + (r << 16) + (g << 8) + b).toString(16).slice(1)}`;
};

const hexToHsl = (hex: string): { h: number; s: number; l: number } => {
  const r = parseInt(hex.slice(1, 3), 16) / 255;
  const g = parseInt(hex.slice(3, 5), 16) / 255;
  const b = parseInt(hex.slice(5, 7), 16) / 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  let h: number, s: number;
  const l = (max + min) / 2;

  if (max === min) {
    h = s = 0; // achromatic
  } else {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);

    switch (max) {
      case r:
        h = (g - b) / d + (g < b ? 6 : 0);
        break;
      case g:
        h = (b - r) / d + 2;
        break;
      case b:
        h = (r - g) / d + 4;
        break;
      default:
        h = 0;
    }
    h /= 6;
  }

  return { h: h * 360, s: s * 100, l: l * 100 };
};

const hslToHex = (h: number, s: number, l: number): string => {
  h = h / 360;
  s = s / 100;
  l = l / 100;

  const hue2rgb = (p: number, q: number, t: number) => {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };

  let r: number, g: number, b: number;

  if (s === 0) {
    r = g = b = l; // achromatic
  } else {
    const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
    const p = 2 * l - q;
    r = hue2rgb(p, q, h + 1 / 3);
    g = hue2rgb(p, q, h);
    b = hue2rgb(p, q, h - 1 / 3);
  }

  const toHex = (c: number) => {
    const hex = Math.round(c * 255).toString(16);
    return hex.length === 1 ? `0${hex}` : hex;
  };

  return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
};

// Default fallback palette
export const getDefaultPalette = (): ColorPalette => ({
  primary: [
    "#e3f2fd",
    "#bbdefb",
    "#90caf9",
    "#64b5f6",
    "#42a5f5",
    "#2196f3",
    "#1e88e5",
    "#1976d2",
    "#1565c0",
  ],
  accent: "#f50057",
  background: "#ffffff",
  surface: "#f5f5f5",
});

// Alias for backwards compatibility
export const extractPalette = extractColorsFromImage;

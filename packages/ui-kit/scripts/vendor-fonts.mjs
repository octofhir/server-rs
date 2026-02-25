import { cp, mkdir, rm, stat } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const kitRoot = resolve(scriptDir, "..");
const outputRoot = resolve(kitRoot, "src/shared/assets/fonts");

const subsets = ["latin", "latin-ext", "cyrillic", "cyrillic-ext"];

const fontDefs = [
    {
        packageName: "@fontsource-variable/manrope",
        folder: "manrope",
        filePrefix: "manrope",
    },
    {
        packageName: "@fontsource-variable/rubik",
        folder: "rubik",
        filePrefix: "rubik",
    },
    {
        packageName: "@fontsource-variable/jetbrains-mono",
        folder: "jetbrains-mono",
        filePrefix: "jetbrains-mono",
    },
];

async function assertExists(path) {
    try {
        await stat(path);
    } catch {
        throw new Error(`Missing file: ${path}`);
    }
}

async function copyFontFamily(def) {
    const sourceDir = resolve(kitRoot, "node_modules", def.packageName);
    const sourceFilesDir = resolve(sourceDir, "files");
    const targetDir = resolve(outputRoot, def.folder);

    await assertExists(sourceFilesDir);
    await mkdir(targetDir, { recursive: true });

    for (const subset of subsets) {
        const fileName = `${def.filePrefix}-${subset}-wght-normal.woff2`;
        await cp(resolve(sourceFilesDir, fileName), resolve(targetDir, fileName));
    }

    await cp(resolve(sourceDir, "LICENSE"), resolve(targetDir, "LICENSE"));
}

async function main() {
    await rm(outputRoot, {
        recursive: true,
        force: true,
        maxRetries: 5,
        retryDelay: 100,
    });
    await mkdir(outputRoot, { recursive: true });

    for (const def of fontDefs) {
        await copyFontFamily(def);
    }

    console.log("Fonts synced to packages/ui-kit/src/shared/assets/fonts");
}

main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
});

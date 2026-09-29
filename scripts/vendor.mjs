// Copie Plotly.js (installé par npm) dans ui/vendor/, servi par l'application
// sans accès Internet. Lancé automatiquement avant `tauri dev` / `tauri build`.
import { copyFileSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(import.meta.url);
const source = require.resolve("plotly.js-dist-min/plotly.min.js");
const target = join(root, "ui", "vendor", "plotly.min.js");

mkdirSync(dirname(target), { recursive: true });
copyFileSync(source, target);
console.log(`Plotly.js copié dans ${target}`);

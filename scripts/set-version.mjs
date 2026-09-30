// Fixe la version de l'application partout où elle est écrite :
//   node scripts/set-version.mjs 2.1.0   (un préfixe « v » est accepté)
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const version = (process.argv[2] || "").replace(/^v/, "");
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`Version invalide : « ${process.argv[2] ?? ""} » (attendu : 2.1.0)`);
  process.exit(1);
}
const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const replaceIn = (file, pattern, replacement) => {
  const path = join(root, file);
  const before = readFileSync(path, "utf8");
  // Tester le motif (et non comparer avant/après) : la version peut déjà être la bonne.
  if (!pattern.test(before)) throw new Error(`Version introuvable dans ${file}`);
  writeFileSync(path, before.replace(pattern, replacement));
};

// \r?\n : sous Windows, git extrait les fichiers avec des fins de ligne CRLF.
replaceIn("Cargo.toml", /(\[workspace\.package\]\r?\nversion = )"[^"]*"/, `$1"${version}"`);
replaceIn("src-tauri/tauri.conf.json", /("version": )"[^"]*"/, `$1"${version}"`);
replaceIn("package.json", /("version": )"[^"]*"/, `$1"${version}"`);
console.log(`Version fixée à ${version}`);

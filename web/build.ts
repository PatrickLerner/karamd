// Build the SPA into dist/ with content-hashed asset URLs so a deploy that
// changes the JS or CSS gets a fresh URL and is picked up on a normal reload
// (no manual hard refresh). karamd's web server caches hashed assets immutably
// and revalidates index.html — see `cache_control_for` in src/web.rs.
import { createHash } from "node:crypto";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";

const dist = "dist";

await rm(dist, { recursive: true, force: true });
await mkdir(dist, { recursive: true });

// 1. Bundle the JS entry. Bun content-hashes it (and any split chunks) and
//    rewrites the internal import references, so code splitting stays correct.
const result = await Bun.build({
  entrypoints: ["src/main.tsx"],
  outdir: dist,
  minify: true,
  // Match the old `bun build --production`: production React drops its dev-only
  // code, roughly a third smaller than the development bundle.
  define: { "process.env.NODE_ENV": JSON.stringify("production") },
  naming: "[name]-[hash].[ext]",
});
if (!result.success) {
  for (const log of result.logs) console.error(log);
  process.exit(1);
}
const entry = result.outputs.find((o) => o.kind === "entry-point");
if (!entry) {
  console.error("bun build produced no entry-point output");
  process.exit(1);
}
const jsName = entry.path.split("/").pop()!;

// 2. Hash the standalone stylesheet ourselves. It is linked from index.html,
//    not imported by the JS, so bun does not process it — which is what we want:
//    its url("fonts/…") references stay intact and resolve against dist/.
const css = await readFile("src/styles.css");
const cssHash = createHash("sha256").update(css).digest("hex").slice(0, 8);
const cssName = `styles-${cssHash}.css`;
await writeFile(`${dist}/${cssName}`, css);

// 3. Static assets (fonts, favicon) copied verbatim.
await cp("public", dist, { recursive: true });

// 4. Point index.html at the hashed asset URLs.
let html = await readFile("index.html", "utf8");
html = html
  .replace("./main.js", `./${jsName}`)
  .replace("./styles.css", `./${cssName}`);
await writeFile(`${dist}/index.html`, html);

console.log(`built dist/: ${jsName}, ${cssName}`);

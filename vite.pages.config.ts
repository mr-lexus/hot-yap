import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const env =
  (
    globalThis as typeof globalThis & {
      process?: { env?: Record<string, string | undefined> };
    }
  ).process?.env ?? {};
const repository = env.GITHUB_REPOSITORY?.split("/")[1] ?? "hot-yap";
// Keep local previews on the same sub-path as the GitHub Pages deployment.
// An explicit PAGES_BASE still allows a root-mounted preview when needed.
const base = env.PAGES_BASE ?? `/${repository}/`;

const root = fileURLToPath(new URL("./website", import.meta.url));
const publicDir = fileURLToPath(new URL("./public", import.meta.url));
const outDir = fileURLToPath(new URL("./dist-pages", import.meta.url));

export default defineConfig({
  root,
  base,
  publicDir,
  build: {
    outDir,
    emptyOutDir: true,
    rollupOptions: {
      input: [
        fileURLToPath(new URL("./website/index.html", import.meta.url)),
        fileURLToPath(new URL("./website/ru/index.html", import.meta.url)),
      ],
    },
  },
});

import { defineConfig } from "vite";

const env =
  (
    globalThis as typeof globalThis & {
      process?: { env?: Record<string, string | undefined> };
    }
  ).process?.env ?? {};
const repository = env.GITHUB_REPOSITORY?.split("/")[1] ?? "hot-yap";
const base = env.PAGES_BASE ?? (env.GITHUB_ACTIONS ? `/${repository}/` : "/");

export default defineConfig({
  root: new URL("./website", import.meta.url).pathname,
  base,
  publicDir: new URL("./public", import.meta.url).pathname,
  build: {
    outDir: new URL("./dist-pages", import.meta.url).pathname,
    emptyOutDir: true,
    rollupOptions: {
      input: [
        new URL("./website/index.html", import.meta.url).pathname,
        new URL("./website/ru/index.html", import.meta.url).pathname,
      ],
    },
  },
});

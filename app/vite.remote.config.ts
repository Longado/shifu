import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";

export default defineConfig({
  root: fileURLToPath(new URL("./remote/", import.meta.url)),
  publicDir: "public",
  build: {
    target: "safari16",
    outDir: fileURLToPath(new URL("../remote/web-dist/", import.meta.url)),
    emptyOutDir: true,
  },
});

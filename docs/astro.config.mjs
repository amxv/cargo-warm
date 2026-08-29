import { defineConfig } from "astro/config";
import zuedocs from "zuedocs/astro";

export default defineConfig({
  output: "static",
  site: "https://cargowarm.ashray.xyz",
  integrations: [zuedocs()]
});

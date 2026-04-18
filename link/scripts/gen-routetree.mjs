#!/usr/bin/env node
// Generate src/routeTree.gen.ts before tsc runs.
//
// The TanStack Router Vite plugin produces this file as a side effect of
// `vite dev`/`vite build`, but our `npm run build` runs `tsc -b` *first* for
// type safety. On a fresh checkout (CI, fresh clone) the gen file does not
// exist yet, so tsc fails before vite ever gets a chance to write it. We use
// the lower-level `@tanstack/router-generator` (already pinned via the plugin)
// so the inputs match what `vite build` would emit.

import { fileURLToPath } from "node:url";
import path from "node:path";
import { Generator, getConfig } from "@tanstack/router-generator";

const here = path.dirname(fileURLToPath(import.meta.url));
const linkRoot = path.resolve(here, "..");

const config = getConfig(
  {
    routesDirectory: path.join(linkRoot, "src/routes"),
    generatedRouteTree: path.join(linkRoot, "src/routeTree.gen.ts"),
    target: "react",
    disableLogging: true,
  },
  linkRoot,
);

const gen = new Generator({ config, root: linkRoot });
await gen.run();
console.log("routeTree.gen.ts generated");

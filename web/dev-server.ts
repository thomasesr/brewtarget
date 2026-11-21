// SPDX-License-Identifier: GPL-3.0-or-later
//
// dev-server.ts is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

// Development server.  Serves the frontend with hot reload and proxies /api to the
// Rust backend, so the dev experience matches production (where bt-server serves
// both).  Production never runs this file -- see PLAN.md Phase 9.

import index from "./index.html";

const API = process.env.BT_API_URL ?? "http://127.0.0.1:8080";

const server = Bun.serve({
  port: Number(process.env.PORT ?? 3000),
  development: true,
  routes: {
    "/api/*": (req) => {
      const url = new URL(req.url);
      return fetch(`${API}${url.pathname}${url.search}`, req);
    },
    "/*": index,
  },
});

console.log(`web dev server on ${server.url} (API -> ${API})`);

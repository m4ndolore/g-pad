#!/usr/bin/env node
// Dev-only shim: expose Vellum's Cloudflare AI Gateway (BYOK OpenRouter) as a
// plain OpenAI-compatible endpoint on localhost, so riddle's oracle can run
// on the dev machine without a raw provider key. Reads CF_AIG_* from
// ~/Dev/vellum/.env.local. Streams straight through.
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import os from "node:os";

const envFile = path.join(os.homedir(), "Dev/vellum/.env.local");
const env = Object.fromEntries(
  fs.readFileSync(envFile, "utf8").split("\n").filter((l) => l.includes("="))
    .map((l) => [l.slice(0, l.indexOf("=")).trim(), l.slice(l.indexOf("=") + 1).trim()]),
);
const base = env.CF_AIG_URL.replace(/\/$/, "");
const upstream = base.endsWith("/chat/completions") ? base : `${base}/chat/completions`;
const alias = env.CF_AIG_BYOK_ALIAS || "anymouse";

http.createServer(async (req, res) => {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const body = JSON.parse(Buffer.concat(chunks).toString());
  if (body.model && !body.model.startsWith("openrouter/")) {
    body.model = `openrouter/${body.model}`;
  }
  try {
    const up = await fetch(upstream, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "cf-aig-authorization": `Bearer ${env.CF_AIG_TOKEN}`,
        "cf-aig-byok-alias": alias,
      },
      body: JSON.stringify(body),
    });
    res.writeHead(up.status, { "Content-Type": up.headers.get("content-type") || "text/event-stream" });
    for await (const chunk of up.body) res.write(chunk);
    res.end();
  } catch (e) {
    res.writeHead(502).end(JSON.stringify({ error: String(e) }));
  }
}).listen(8917, "127.0.0.1", () => console.log(`shim: 127.0.0.1:8917 -> ${upstream} (alias ${alias})`));

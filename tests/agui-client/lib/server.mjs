// Avvia il vero `aiw serve` (binario compilato dal workspace) in una cartella dati temporanea, lo punta a un
// provider finto e dà un piccolo client REST per prepararlo e per rispondere alle approvazioni, come farebbe
// un'app. Niente scorciatoie nel binario di produzione: tutto passa dalle API pubbliche.

import { spawn } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");

/** Il binario `aiw`: `AIW_BIN`, altrimenti quello del target di cargo (`CARGO_TARGET_DIR` o `target/`). */
export function aiwBinary() {
  const exe = process.platform === "win32" ? "aiw.exe" : "aiw";
  const candidates = [
    process.env.AIW_BIN,
    process.env.CARGO_TARGET_DIR && path.join(process.env.CARGO_TARGET_DIR, "debug", exe),
    path.join(repoRoot, "target", "debug", exe),
  ].filter(Boolean);
  const found = candidates.find((c) => fs.existsSync(c));
  if (!found) throw new Error(`binario aiw non trovato (provati: ${candidates.join(", ")}): compila con «cargo build -p aiw-cli» o imposta AIW_BIN`);
  return found;
}

function freePort() {
  return new Promise((ok, fail) => {
    const s = net.createServer();
    s.once("error", fail);
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => ok(port));
    });
  });
}

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

/**
 * @param {{providerBase: string}} opts `providerBase`: URL `/v1` del provider finto.
 */
export async function startServer({ providerBase }) {
  const data = fs.mkdtempSync(path.join(os.tmpdir(), "aiw-agui-data-"));
  const workspace = fs.mkdtempSync(path.join(os.tmpdir(), "aiw-agui-ws-"));
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const log = [];
  let child;
  let exited = true;

  const spawnServer = () => {
    exited = false;
    child = spawn(aiwBinary(), ["--data", data, "--workspace", workspace, "serve", "--port", String(port)], {
      env: { ...process.env, RUST_LOG: "warn" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    for (const stream of [child.stdout, child.stderr]) stream.on("data", (b) => log.push(String(b)));
    child.on("exit", () => (exited = true));
  };

  const api = async (method, p, body, headers = {}) => {
    const res = await fetch(base + p, {
      method,
      headers: { "content-type": "application/json", ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const textBody = await res.text();
    let json;
    try {
      json = textBody ? JSON.parse(textBody) : undefined;
    } catch {
      json = undefined;
    }
    return { status: res.status, json, text: textBody, headers: res.headers };
  };

  const stopChild = async () => {
    if (!exited) {
      child.kill("SIGTERM");
      await Promise.race([new Promise((ok) => child.once("exit", ok)), sleep(5000)]);
      if (!exited) child.kill("SIGKILL");
    }
  };

  // Pronto quando risponde l'elenco degli agenti.
  const waitReady = async () => {
    const deadline = Date.now() + 30_000;
    for (;;) {
      if (exited) throw new Error(`aiw è terminato all'avvio:\n${log.join("")}`);
      try {
        if ((await api("GET", "/api/agents")).status === 200) return;
      } catch {
        /* non ancora in ascolto */
      }
      if (Date.now() > deadline) throw new Error(`aiw non risponde dopo 30 s:\n${log.join("")}`);
      await sleep(100);
    }
  };

  const server = {
    base,
    data,
    workspace,
    api,
    log: () => log.join(""),
    async stop() {
      await stopChild();
      fs.rmSync(data, { recursive: true, force: true });
      fs.rmSync(workspace, { recursive: true, force: true });
    },
    /** Ferma il processo e lo rilancia sugli stessi dati e sulla stessa porta: serve a provare ciò che è durevole. */
    async restart() {
      await stopChild();
      spawnServer();
      await waitReady();
    },
  };

  spawnServer();
  await waitReady();

  // Il provider finto: runtime `remote-openai` + profilo `fake`, e gli agenti che servono puntati al profilo.
  const must = (r, what) => {
    if (r.status >= 300) throw new Error(`${what}: HTTP ${r.status} ${r.text}`);
    return r;
  };
  must(await api("PUT", "/api/runtimes/remote-openai/settings", { base_url: providerBase }), "impostazioni del runtime");
  must(
    await api("PUT", "/api/profiles/fake", { id: "fake", name: "Provider finto", runtime_id: "remote-openai", model: "fake-model", params: {} }),
    "profilo",
  );
  const agents = must(await api("GET", "/api/agents"), "agenti").json;
  for (const a of agents) {
    must(await api("PUT", `/api/agents/${a.id}`, { ...a, inference_profile: "fake" }), `agente ${a.id}`);
  }
  return server;
}

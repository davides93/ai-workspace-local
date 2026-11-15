#!/usr/bin/env node
// Captures of docs/design/screenshots, taken from the mockup build (the real UI against the demo transport) so that they can be
// made again whenever the interface changes: one script, the same pages, the same window sizes, a frozen clock (the demo's
// «4 min fa» does not depend on the day it is run) and the bundled fonts. It builds nothing: run `npm run build:mockup` first.
//
//   npm run shots:design                              every capture into docs/design/screenshots
//   npm run shots:design -- --only=chat-conferma      only those (comma separated)
//   npm run shots:design -- --out=/tmp/shots          somewhere else (a folder that exists or can be made)
//   npm run shots:design -- --mockup                  also copy dist-mockup/index.html to docs/design/mockup/index.html
//   npm run shots:design -- --list                    the names, with what each one shows
//
// `AIW_SHOTS_HTML` points the script at another build. Chromium comes from Playwright (`npx playwright install chromium`) or
// from `AIW_CHROMIUM`. Nothing here talks to the network.
import { createRequire } from "node:module";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const { chromium } = createRequire(import.meta.url)("playwright");
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const htmlPath = path.resolve(process.env.AIW_SHOTS_HTML ?? path.join(root, "dist-mockup", "index.html"));
const arg = (name) => process.argv.find((a) => a === `--${name}` || a.startsWith(`--${name}=`));
const argValue = (name) => arg(name)?.split("=").slice(1).join("=");

const DESKTOP = { width: 1440, height: 900 };
const PHONE = { width: 390, height: 844 };
/** The clock of every capture: the demo's relative times («4 min fa», «Ieri») come out the same on any day. */
const NOW = new Date("2026-11-15T10:00:00+01:00");

/**
 * `ui`: the interface mode the demo starts in (`developer` shows every section, the default is the simple one with the chat only).
 * `scenario`: see src/shared/api/mock/scenario.ts. `prepare`: what a person does before the page is worth a picture.
 */
const SHOTS = [
  { name: "chat-completata", about: "Developer: a finished run with its tool card, the run panel on the right", size: DESKTOP, route: "/chat/ses_pdf", ui: "developer" },
  {
    name: "chat-conferma", about: "Developer: the shell waits for the person's approval (the picture of the README)", size: DESKTOP, route: "/chat/ses_pdf", ui: "developer",
    async prepare(page) {
      await page.getByRole("textbox", { name: "Messaggio" }).fill("Ordina i PDF in Downloads");
      await page.getByRole("button", { name: "Invia" }).click();
      await page.getByRole("log", { name: "Conversazione" }).getByRole("article", { name: "shell" }).waitFor({ timeout: 20000 });
    },
  },
  { name: "mobile-chat", about: "Simple interface on a phone: the chat and nothing else", size: PHONE, route: "/chat/ses_pdf" },
  { name: "chat-semplice", about: "Simple interface on a desktop: the list of chats and the conversation", size: DESKTOP, route: "/chat/ses_pdf" },
  { name: "chat-scuro", about: "Simple interface in the dark theme", size: DESKTOP, route: "/chat/ses_pdf", theme: "dark" },
  {
    name: "chat-codice", about: "Code blocks coloured by language in a long conversation (scenario long-chat)", size: DESKTOP, route: "/chat/ses_long", scenario: "long-chat",
    async prepare(page) {
      await page.locator(".ws-log-inner .tok-kw").first().waitFor({ timeout: 10000 });
      await page.evaluate(() => {
        const row = [...document.querySelectorAll(".ws-log-inner > .ws-msg")].find((r) => r.textContent?.includes("Domanda numero 76:"));
        row?.scrollIntoView({ block: "start" });
      });
      await page.waitForTimeout(400);
    },
  },
  { name: "modelli", about: "Developer: the models that can run on this device", size: DESKTOP, route: "/models", ui: "developer" },
  { name: "runtime", about: "Developer: the runtimes", size: DESKTOP, route: "/runtimes", ui: "developer" },
  { name: "impostazioni", about: "Developer: the settings, in layers", size: DESKTOP, route: "/settings", ui: "developer" },
  { name: "prima-accensione", about: "A new installation (scenario first-run): the preparation of the first model", size: DESKTOP, route: "/chat", scenario: "first-run" },
  {
    name: "connessione-persa", about: "The workspace does not answer (scenario offline)", size: DESKTOP, route: "/chat", scenario: "offline",
    async prepare(page) {
      await page.getByText("Connessione al workspace persa").waitFor({ timeout: 15000 });
    },
  },
  {
    name: "accoppiamento", about: "A browser that is not paired yet (scenario unpaired)", size: PHONE, route: "/", scenario: "unpaired",
    async prepare(page) {
      await page.getByRole("heading", { name: "Accoppia questo dispositivo" }).waitFor({ timeout: 15000 });
    },
  },
];

if (arg("list")) {
  for (const s of SHOTS) console.log(`${s.name.padEnd(18)} ${s.size.width}x${s.size.height}  ${s.about}`);
  process.exit(0);
}
if (!existsSync(htmlPath)) {
  console.error(`Manca ${path.relative(process.cwd(), htmlPath)}: esegui prima "npm run build:mockup".`);
  process.exit(2);
}
const only = argValue("only")?.split(",").map((s) => s.trim()).filter(Boolean);
const chosen = SHOTS.filter((s) => !only || only.includes(s.name));
const unknown = (only ?? []).filter((n) => !SHOTS.some((s) => s.name === n));
if (unknown.length > 0 || chosen.length === 0) {
  console.error(`Nessuna cattura con questo nome: ${unknown.join(", ") || "(vuoto)"}. Quelle che esistono: ${SHOTS.map((s) => s.name).join(", ")}.`);
  process.exit(2);
}
const outDir = path.resolve(argValue("out") ?? path.join(root, "..", "..", "docs", "design", "screenshots"));
mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch(process.env.AIW_CHROMIUM ? { executablePath: process.env.AIW_CHROMIUM } : {});
const problems = [];
try {
  for (const shot of chosen) {
    const context = await browser.newContext({ viewport: shot.size, locale: "it-IT", colorScheme: shot.theme ?? "light", deviceScaleFactor: 1, reducedMotion: "reduce" });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
    await page.clock.setFixedTime(NOW);
    await page.addInitScript((theme) => {
      try {
        localStorage.setItem("aiw.theme", theme);
      } catch {
        /* storage unavailable */
      }
    }, shot.theme ?? "light");
    const url = new URL(pathToFileURL(htmlPath).href);
    if (shot.scenario) url.searchParams.set("scenario", shot.scenario);
    if (shot.ui) url.searchParams.set("ui", shot.ui);
    url.hash = shot.route;
    await page.goto(url.href);
    await page.waitForSelector("#main", { timeout: 15000 });
    await page.waitForFunction(() => !document.querySelector('[aria-label="Caricamento"]'), null, { timeout: 15000 });
    await shot.prepare?.(page);
    await page.addStyleTag({ content: "*,*::before,*::after{transition:none!important;animation:none!important}" });
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(500);
    const file = path.join(outDir, `${shot.name}.png`);
    await page.screenshot({ path: file, caret: "hide", animations: "disabled" });
    console.log(`${shot.name}.png  ${shot.size.width}x${shot.size.height}`);
    if (errors.length > 0) problems.push(`${shot.name}: ${errors.join(" | ")}`);
    await context.close();
  }
} finally {
  await browser.close();
}
if (arg("mockup")) {
  const target = path.join(root, "..", "..", "docs", "design", "mockup", "index.html");
  mkdirSync(path.dirname(target), { recursive: true });
  copyFileSync(htmlPath, target);
  console.log(`mockup → ${path.relative(process.cwd(), target)}`);
}
if (problems.length > 0) {
  console.error(`\nErrori nella console durante le catture:\n${problems.map((p) => `  - ${p}`).join("\n")}`);
  process.exit(1);
}

import type { Page } from "@playwright/test";
import { expect, openApp, test } from "../support/app";

// Dictating an instruction for a background task. A headless browser has no microphone (asking for one waits for a
// permission nobody gives), so the page gets a fake one: a stream that is quiet for the first frame and then a tone
// (`tone: false`: silence all along). Everything after it is the real path of the box: voice-activity detection, the WAV, the
// demo's speech to text (a fixed sentence), the words put in the box to be read, and the send with the voice mark.
async function fakeMicrophone(page: Page, tone: boolean) {
  await page.addInitScript((withTone) => {
    class Node {
      onaudioprocess: ((e: { inputBuffer: { getChannelData: () => Float32Array } }) => void) | null = null;
      connect() {}
      disconnect() {}
    }
    class FakeAudioContext {
      state = "running";
      sampleRate = 16_000;
      destination = {};
      resume = () => Promise.resolve();
      close = () => Promise.resolve();
      createMediaStreamSource = () => new Node();
      createScriptProcessor() {
        const node = new Node();
        let n = 0;
        setInterval(() => {
          const frame = new Float32Array(4096);
          if (withTone && n >= 1) for (let i = 0; i < frame.length; i++) frame[i] = 0.3 * Math.sin(i / 8);
          n++;
          node.onaudioprocess?.({ inputBuffer: { getChannelData: () => frame } });
        }, 40);
        return node;
      }
    }
    Object.defineProperty(window, "AudioContext", { value: FakeAudioContext, configurable: true });
    Object.defineProperty(navigator.mediaDevices, "getUserMedia", { value: async () => ({ getTracks: () => [{ stop() {} }] }), configurable: true });
  }, tone);
}

const run = (page: Page) => page.getByRole("region", { name: "Lavori in background" }).getByRole("article", { name: "Archiviare le fatture di ottobre" });

test("the dictated words go in the box to be read, and are sent with the voice mark", async ({ page }) => {
  await fakeMicrophone(page, true);
  await openApp(page, "/inbox", { mode: "developer" });
  const card = run(page);
  const say = card.getByRole("textbox", { name: "Istruzione" });
  const given = card.getByRole("list", { name: "Istruzioni date" });
  await say.fill("Prima questo.");
  await card.getByRole("button", { name: "Detta l'istruzione" }).click();
  const stop = card.getByRole("button", { name: "Ferma la dettatura" });
  await expect(stop).toHaveAttribute("aria-pressed", "true");
  await page.waitForTimeout(600);
  await stop.click();
  // Nothing is sent by itself: the words are in the box after what was there, with a reminder to read them first.
  await expect(say).toHaveValue("Prima questo. Quali file ho modificato oggi nel workspace?");
  await expect(card.getByText("Controlla il testo detto prima di inviarlo.")).toBeVisible();
  await expect(given.getByText("a voce")).toHaveCount(0);
  await card.getByRole("button", { name: "Invia", exact: true }).click();
  const sent = given.getByRole("listitem").filter({ hasText: "Prima questo. Quali file ho modificato oggi nel workspace?" });
  await expect(sent.getByText("a voce", { exact: true })).toBeVisible();
  await expect(say).toHaveValue("");
  await expect(card.getByRole("button", { name: "Detta l'istruzione" })).toBeEnabled();
});

test("a silence leaves the box as it was and says so", async ({ page }) => {
  await fakeMicrophone(page, false);
  await openApp(page, "/inbox", { mode: "developer" });
  const card = run(page);
  const say = card.getByRole("textbox", { name: "Istruzione" });
  await say.fill("Resta qui.");
  await card.getByRole("button", { name: "Detta l'istruzione" }).click();
  const stop = card.getByRole("button", { name: "Ferma la dettatura" });
  await expect(stop).toBeVisible();
  await page.waitForTimeout(400);
  await stop.click();
  await expect(page.getByText("Non ho sentito nulla", { exact: false })).toBeVisible();
  await expect(say).toHaveValue("Resta qui.");
  await expect(card.getByRole("button", { name: "Detta l'istruzione" })).toBeEnabled();
});

test("a text typed by hand goes without the voice mark", async ({ page }) => {
  await openApp(page, "/inbox", { mode: "developer" });
  const card = run(page);
  await card.getByRole("textbox", { name: "Istruzione" }).fill("Scritto a mano.");
  await card.getByRole("button", { name: "Invia", exact: true }).click();
  const sent = card.getByRole("list", { name: "Istruzioni date" }).getByRole("listitem").filter({ hasText: "Scritto a mano." });
  await expect(sent).toBeVisible();
  await expect(sent.getByText("a voce")).toHaveCount(0);
});

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Composer } from "./Composer";
import type { VoiceController } from "../useVoice";

// Dictation in the simple interface is a labelled button; in the developer interface it stays the bare microphone icon.
const voice = { supported: true, available: true, dictation: "off", level: 0, status: undefined, toggleDictation: vi.fn(), toggleConvo: vi.fn(), convo: "off" } as unknown as VoiceController;
const mount = (simple: boolean) => render(
  <Composer value="" onChange={() => {}} onSend={() => {}} onStop={() => {}} running={false} disabled={false} placeholder="" focusKey={null} voice={voice} simple={simple} />,
);

describe("Composer dictation", () => {
  it("shows a «Detta» button in the simple interface", () => {
    mount(true);
    expect(screen.getByRole("button", { name: "Detta" })).toBeInTheDocument();
  });
  it("keeps the microphone icon in the developer interface", () => {
    mount(false);
    expect(screen.queryByRole("button", { name: "Detta" })).toBeNull();
    expect(screen.getByRole("button", { name: "Detta il messaggio" })).toBeInTheDocument();
  });
});

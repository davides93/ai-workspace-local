import { catalog, models, runtimes, sessionMessages, sessions } from "./fixtures";

/**
 * Named starting states of the demo, picked with `?scenario=<name>` in the address of the mockup.
 * The default demo is a well-used workspace; the end-to-end tests (`e2e/`) also need the other end,
 * so a scenario only empties what a new installation does not have yet. Nothing is random.
 *
 * - `first-run`: a runtime that is up, no model on disk, no conversation: what the first start of
 *   the simple interface looks like before "Prepara tutto".
 */
export const MOCK_SCENARIOS = ["first-run"] as const;
export type MockScenario = (typeof MOCK_SCENARIOS)[number];

export const isMockScenario = (v: unknown): v is MockScenario => (MOCK_SCENARIOS as readonly unknown[]).includes(v);

/** Mutates the demo fixtures in place; call once, before the transport answers its first request. */
export function applyMockScenario(name: MockScenario): void {
  if (name === "first-run") {
    models.length = 0;
    for (const c of catalog) c.installed = false;
    for (const r of runtimes) r.loaded_models = [];
    sessions.length = 0;
    for (const id of Object.keys(sessionMessages)) delete sessionMessages[id];
  }
}

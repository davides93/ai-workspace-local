import { describe, expect, it } from "vitest";
import type { AgentProfile, TeamDef } from "../../../shared/api/types";
import { agents as demoAgents, teams as demoTeams } from "../../../shared/api/mock/fixtures";
import {
  blankTeam, duplicateTeam, hasErrors, idForNew, isTeamAgentId, MAX_MEMBERS, normalizeTeam, remoteOptions, teamAgentId, teamAsAgent, teamIdOf, validateTeam,
} from "./teams";

const ctx = { agentIds: ["a", "b", "c"], agentsLoaded: true, otherNames: [] as string[] };
const team = (patch: Partial<TeamDef> = {}): TeamDef => ({
  id: "t", name: "Squadra", description: "", strategy: "pipeline", members: [{ agent: "a", role: "" }, { agent: "b", role: "" }], ...patch,
});
const messages = (t: TeamDef, c = ctx) => validateTeam(t, c).filter((i) => i.level === "error").map((i) => i.message);

describe("team agent ids", () => {
  it("round-trips through the `team:` prefix", () => {
    expect(teamAgentId("rev")).toBe("team:rev");
    expect(isTeamAgentId("team:rev")).toBe(true);
    expect(isTeamAgentId("operator")).toBe(false);
    expect(teamIdOf("team:rev")).toBe("rev");
    expect(teamIdOf("operator")).toBeNull();
  });
  it("derives a unique id from the name and never the reserved one", () => {
    expect(idForNew("Revisione del codice", [])).toBe("revisione-del-codice");
    expect(idForNew("Revisione", [{ ...team(), id: "revisione" }])).toBe("revisione-2");
    expect(idForNew("New", [])).toBe("new-2");
  });
});

describe("normalizeTeam", () => {
  it("trims, and keeps only what the strategy uses", () => {
    const n = normalizeTeam(team({ name: "  X ", strategy: "pipeline", supervisor: "c", aggregator: "c", members: [{ agent: " a ", role: " r " }] }));
    expect(n).toEqual({ id: "t", name: "X", description: "", strategy: "pipeline", members: [{ agent: "a", role: "r" }] });
    expect(normalizeTeam(team({ strategy: "supervisor", supervisor: "c", aggregator: "a" }))).toMatchObject({ supervisor: "c" });
    expect(normalizeTeam(team({ strategy: "supervisor", supervisor: "c", aggregator: "a" }))).not.toHaveProperty("aggregator");
    expect(normalizeTeam(team({ strategy: "parallel", aggregator: "c" }))).toMatchObject({ aggregator: "c" });
  });
  it("drops empty budget limits", () => {
    expect(normalizeTeam(team({ budget: { max_tokens: Number.NaN } }))).not.toHaveProperty("budget");
    expect(normalizeTeam(team({ budget: { max_turns: 4 } }))).toMatchObject({ budget: { max_turns: 4 } });
  });
});

describe("validateTeam", () => {
  it("accepts a good team, one of each strategy", () => {
    expect(messages(team())).toEqual([]);
    expect(messages(team({ strategy: "supervisor", supervisor: "c" }))).toEqual([]);
    expect(messages(team({ strategy: "parallel", aggregator: "c" }))).toEqual([]);
  });
  it("needs a name and 1 to 8 distinct members", () => {
    expect(messages(team({ name: " " }))).toEqual(["Dai un nome al team."]);
    expect(messages(team({ members: [] }))).toEqual(["Aggiungi almeno un membro."]);
    const many = Array.from({ length: MAX_MEMBERS + 1 }, (_, i) => ({ agent: `remote:r${i}`, role: "" }));
    expect(messages(team({ members: many }))).toEqual([`Un team ha al massimo ${MAX_MEMBERS} membri.`]);
    expect(messages(team({ members: [{ agent: "a", role: "" }, { agent: "a", role: "" }] }))).toEqual(["L'agente «a» compare più volte."]);
  });
  it("checks that the agents exist, except remote ones", () => {
    expect(messages(team({ members: [{ agent: "zz", role: "" }] }))).toEqual(["L'agente «zz» non esiste più: toglilo o sostituiscilo."]);
    expect(messages(team({ members: [{ agent: "remote:r1", role: "" }] }))).toEqual([]);
    // The list of agents has not arrived: nothing is flagged.
    expect(messages(team({ members: [{ agent: "zz", role: "" }] }), { ...ctx, agentsLoaded: false })).toEqual([]);
  });
  it("needs a supervisor that is not a member, and an existing aggregator", () => {
    expect(messages(team({ strategy: "supervisor" }))).toEqual(["Scegli il supervisore."]);
    expect(messages(team({ strategy: "supervisor", supervisor: "a" }))).toEqual(["Il supervisore non può essere anche un membro."]);
    expect(messages(team({ strategy: "supervisor", supervisor: "zz" }))).toEqual(["Il supervisore «zz» non esiste più."]);
    expect(messages(team({ strategy: "parallel", aggregator: "zz" }))).toEqual(["L'aggregatore «zz» non esiste più."]);
  });
  it("flags budget limits under 1", () => {
    expect(messages(team({ budget: { max_tokens: 0 } }))).toHaveLength(1);
    expect(messages(team({ budget: { max_seconds: 1.5 } }))).toHaveLength(1);
    expect(hasErrors(validateTeam(team({ budget: { max_turns: 3 } }), ctx))).toBe(false);
  });
  it("only warns about a name that is taken", () => {
    const issues = validateTeam(team(), { ...ctx, otherNames: ["squadra"] });
    expect(issues).toEqual([{ field: "name", level: "warn", message: "Esiste già un altro team con questo nome." }]);
  });
});

describe("blank and duplicate", () => {
  it("starts as an empty supervisor team", () => {
    expect(blankTeam()).toMatchObject({ id: "", name: "", strategy: "supervisor", members: [] });
  });
  it("copies with a new name and no id", () => {
    const copy = duplicateTeam(team(), [team()]);
    expect(copy).toMatchObject({ id: "", name: "Copia di Squadra" });
    expect(duplicateTeam(team(), [team(), { ...team(), name: "Copia di Squadra" }]).name).toBe("Copia di Squadra (2)");
    copy.members[0].role = "x";
    expect(team().members[0].role).toBe("");
  });
});

describe("teamAsAgent", () => {
  const agent = (id: string, mode: AgentProfile["mode"], sandbox: AgentProfile["sandbox"]): AgentProfile =>
    ({ id, name: id, description: "", inference_profile: `p-${id}`, system_prompt: "", tools: ["*"], mode, sandbox, max_turns: 5 });
  const all = [agent("a", "plan", "read-only"), agent("b", "auto", "workspace-write"), agent("c", "ask", "read-only")];

  it("runs a supervisor team as its supervisor, under the team's name", () => {
    const a = teamAsAgent(team({ strategy: "supervisor", supervisor: "c", name: "Rev" }), all)!;
    expect(a).toMatchObject({ id: "team:t", name: "Rev", inference_profile: "p-c", mode: "ask", sandbox: "read-only" });
  });
  it("takes the widest mode and sandbox for a team without supervisor", () => {
    const a = teamAsAgent(team(), all)!;
    expect(a).toMatchObject({ id: "team:t", inference_profile: "p-a", mode: "auto", sandbox: "workspace-write" });
  });
  it("has nothing to show when no member is local", () => {
    expect(teamAsAgent(team({ members: [{ agent: "remote:r1", role: "" }] }), all)).toBeNull();
  });
});

describe("remoteOptions", () => {
  it("offers the remote agents as `remote:<id>` choices, flagged as remote", () => {
    const r = { id: "studio", name: "PC dello studio", url: "http://h", token: "", description: "" };
    expect(remoteOptions([r, { ...r, id: "peer-x", name: "Telefono", paired: true }])).toEqual([
      { id: "remote:studio", name: "PC dello studio", remote: true },
      { id: "remote:peer-x", name: "Telefono", remote: true },
    ]);
  });
});

describe("the demo teams", () => {
  it("are valid against the demo agents", () => {
    const c = { agentIds: demoAgents.map((a) => a.id), agentsLoaded: true, otherNames: [] };
    for (const t of demoTeams) expect(validateTeam(t, c).filter((i) => i.level === "error"), t.id).toEqual([]);
  });
});

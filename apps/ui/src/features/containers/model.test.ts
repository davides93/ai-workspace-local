import { describe, expect, it } from "vitest";
import type { ContainerEngine, ContainerStatus } from "../../shared/api/types";
import { DOCKER_COMPOSE_URL, DOCKER_DESKTOP_URL, DOCKER_ENGINE_URL, pollInterval, resources, setupView, withResources } from "./model";

const engine = (o: Partial<ContainerEngine> = {}): ContainerEngine => ({ engine: "docker", installed: true, running: false, compose: false, ...o });
const status = (o: Partial<ContainerStatus>): ContainerStatus => ({ state: "not_installed", engine: null, hint: "install_docker", engines: [], os: "macos", ...o });

describe("setupView", () => {
  it("Docker missing: a link to Docker Desktop, or to the Engine docs on Linux", () => {
    const mac = setupView(status({}));
    expect([mac.tone, mac.title, mac.link?.href]).toEqual(["todo", "containers.manca_titolo", DOCKER_DESKTOP_URL]);
    const linux = setupView(status({ os: "linux" }));
    expect([linux.body, linux.link?.href]).toEqual(["containers.manca_testo_linux", DOCKER_ENGINE_URL]);
  });

  it("installed but off: no link, the step is to start it", () => {
    const v = setupView(status({ state: "engine_stopped", engine: "docker", hint: "start_docker", engines: [engine({ version: "27.3.1" })] }));
    expect([v.tone, v.title, v.link, v.params.version]).toEqual(["todo", "containers.fermo_titolo", undefined, "27.3.1"]);
    expect(setupView(status({ state: "engine_stopped", engine: "docker", os: "linux" })).body).toBe("containers.fermo_testo_linux");
  });

  it("running without compose points to the Compose docs", () => {
    const v = setupView(status({ state: "compose_missing", engine: "podman", hint: "install_compose" }));
    expect([v.title, v.link?.href, v.params.name]).toEqual(["containers.compose_titolo", DOCKER_COMPOSE_URL, "Podman"]);
  });

  it("ready says the engine and its resources, and has nothing to do", () => {
    const v = setupView(status({ state: "ready", engine: "docker", hint: "none", engines: [engine({ running: true, compose: true, version: "27.3.1", cpus: 8, memory_mb: 7836 })] }));
    expect([v.tone, v.link, v.params.resources]).toEqual(["ok", undefined, " (8 CPU · 7,7 GB)"]);
  });

  it("a phone has nothing to install", () => {
    const v = setupView(status({ state: "unsupported", os: "ios", hint: "none" }));
    expect([v.tone, v.link]).toEqual(["info", undefined]);
  });
});

describe("resources and polling", () => {
  it("leaves out what the engine did not say", () => {
    expect(resources(null, null)).toBe("");
    expect(resources(4, undefined)).toBe("4 CPU");
    expect(withResources(undefined, undefined)).toBe("");
  });
  it("asks again until ready or impossible", () => {
    expect(pollInterval(undefined)).toBe(false);
    expect(pollInterval(status({}))).toBe(3_000);
    expect(pollInterval(status({ state: "ready" }))).toBe(false);
    expect(pollInterval(status({ state: "unsupported" }))).toBe(false);
  });
});

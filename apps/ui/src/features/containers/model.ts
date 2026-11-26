import type { ContainerStatus } from "../../shared/api/types";
import type { MessageKey } from "../../shared/i18n";

/**
 * What the guided setup shows for one answer of `GET /api/containers`: one sentence, and at most one thing to do.
 * Pure, so the wording per case is tested without rendering. Nothing here installs or starts anything: the person does it,
 * then asks again (the panel also asks by itself while it is open).
 */
export interface SetupView {
  /** `ok` is the end of the guide; `todo` has a step; `info` has nothing to do here (a phone). */
  tone: "ok" | "todo" | "info";
  title: MessageKey;
  body: MessageKey;
  /** The link the person follows, if any. */
  link?: { label: MessageKey; href: string };
  /** Words filled into `title` and `body`. */
  params: Record<string, string | number>;
}

export const DOCKER_DESKTOP_URL = "https://www.docker.com/products/docker-desktop/";
export const DOCKER_ENGINE_URL = "https://docs.docker.com/engine/install/";
export const DOCKER_COMPOSE_URL = "https://docs.docker.com/compose/install/";

export function setupView(s: ContainerStatus): SetupView {
  const engine = s.engines.find((e) => e.engine === s.engine);
  const name = s.engine === "podman" ? "Podman" : "Docker";
  const params: SetupView["params"] = { name, version: engine?.version ?? "" };
  switch (s.state) {
    case "ready":
      return { tone: "ok", title: "containers.pronto_titolo", body: "containers.pronto_testo", params: { ...params, resources: withResources(engine?.cpus, engine?.memory_mb) } };
    case "compose_missing":
      return { tone: "todo", title: "containers.compose_titolo", body: "containers.compose_testo", link: { label: "containers.compose_link", href: DOCKER_COMPOSE_URL }, params };
    case "engine_stopped":
      return { tone: "todo", title: "containers.fermo_titolo", body: s.os === "linux" ? "containers.fermo_testo_linux" : "containers.fermo_testo", params };
    case "unsupported":
      return { tone: "info", title: "containers.telefono_titolo", body: "containers.telefono_testo", params };
    default:
      return {
        tone: "todo",
        title: "containers.manca_titolo",
        body: s.os === "linux" ? "containers.manca_testo_linux" : "containers.manca_testo",
        link: s.os === "linux" ? { label: "containers.manca_link_linux", href: DOCKER_ENGINE_URL } : { label: "containers.manca_link", href: DOCKER_DESKTOP_URL },
        params,
      };
  }
}

/** « (8 CPU · 7,6 GB)» to put after a sentence, or nothing. */
export function withResources(cpus?: number | null, memoryMb?: number | null): string {
  const r = resources(cpus, memoryMb);
  return r ? ` (${r})` : "";
}

/** «8 CPU · 7,6 GB» or nothing when the engine did not say. */
export function resources(cpus?: number | null, memoryMb?: number | null): string {
  const parts: string[] = [];
  if (cpus) parts.push(`${cpus} CPU`);
  if (memoryMb) parts.push(`${(memoryMb / 1024).toLocaleString("it-IT", { maximumFractionDigits: 1 })} GB`);
  return parts.join(" · ");
}

/** Ask again every few seconds while the person installs or starts Docker; stop once it is ready or cannot be here. */
export function pollInterval(s: ContainerStatus | undefined): number | false {
  return !s || s.state === "ready" || s.state === "unsupported" ? false : 3_000;
}

import type { ModelSearchParams } from "../api/types";

/**
 * Query key factory. Every key starts with its domain so a whole domain can be invalidated with
 * `queryClient.invalidateQueries({ queryKey: qk.models.all() })`. Keys are shared across features
 * (a download that finishes invalidates `models.installed` from the models feature, the home
 * page reads the same entries), so they live here and nowhere else.
 */
export const qk = {
  system: () => ["system"] as const,

  runtimes: {
    all: () => ["runtimes"] as const,
    list: () => ["runtimes", "list"] as const,
    logs: (id: string) => ["runtimes", "logs", id] as const,
    installPlan: (id: string) => ["runtimes", "install-plan", id] as const,
    install: (id: string) => ["runtimes", "install", id] as const,
  },

  models: {
    all: () => ["models"] as const,
    installed: () => ["models", "installed"] as const,
    catalog: () => ["models", "catalog"] as const,
    providers: () => ["models", "providers"] as const,
    search: (p: Omit<ModelSearchParams, "cursor">) => ["models", "search", p] as const,
    remoteAll: () => ["models", "remote"] as const,
    remote: (provider: string, model: string, revision?: string) => ["models", "remote", provider, model, revision ?? ""] as const,
    downloads: () => ["models", "downloads"] as const,
  },

  profiles: () => ["profiles"] as const,
  agents: () => ["agents"] as const,
  teams: () => ["teams"] as const,
  remoteAgents: () => ["remote-agents"] as const,

  sessions: {
    all: () => ["sessions"] as const,
    list: () => ["sessions", "list"] as const,
    detail: (id: string) => ["sessions", "detail", id] as const,
    messages: (id: string) => ["sessions", "messages", id] as const,
    worktree: (id: string) => ["sessions", "worktree", id] as const,
  },

  approvals: () => ["approvals"] as const,
  runEvents: (runId: string) => ["runs", "events", runId] as const,

  workflows: {
    all: () => ["workflows"] as const,
    list: () => ["workflows", "list"] as const,
    runs: () => ["workflows", "runs"] as const,
  },

  tools: () => ["tools"] as const,
  mcp: () => ["mcp", "servers"] as const,

  mesh: () => ["mesh"] as const,
  pairingQr: (address: string | null, code: string | undefined) => ["mesh", "qr", address, code ?? ""] as const,

  voice: () => ["voice"] as const,

  config: () => ["config"] as const,
  update: () => ["update"] as const,
  audit: () => ["audit"] as const,
  policy: () => ["security", "policy"] as const,
  hooks: () => ["security", "hooks"] as const,
};

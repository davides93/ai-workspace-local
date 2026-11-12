import type { DiscoveredDevice, PeerDevice } from "../../../shared/api/types";

export const OS_LABEL: Record<string, string> = { windows: "Windows", macos: "macOS", linux: "Linux", android: "Android", ios: "iOS" };

export const DEFAULT_PORT = 7777;

/** Same normalisation as the engine's peer ids (`peers.rs`): lowercase, runs of other characters become `-`, 32 characters at most. */
export const slug = (s: string) => s.replace(/[^A-Za-z0-9]+/g, "-").toLowerCase().replace(/^-+|-+$/g, "").slice(0, 32);

/** Same normalisation as the engine's peer addresses: scheme `http://` and port 7777 by default, no trailing slash. */
export function normAddress(a: string): string {
  const s = a.trim().replace(/\/+$/, "");
  const url = /^https?:\/\//.test(s) ? s : `http://${s}`;
  const hostPort = url.split("://")[1] ?? "";
  return hostPort.includes(":") && !hostPort.endsWith("]") ? url : `${url}:${DEFAULT_PORT}`;
}

/** Is this device found on the network already one of the paired peers? */
export function isPaired(peers: Pick<PeerDevice, "id" | "address">[], d: Pick<DiscoveredDevice, "id" | "address">): boolean {
  return peers.some((p) => normAddress(p.address) === normAddress(d.address) || p.id === slug(d.id));
}

/** The 6 digits of a pairing code, whatever grouping the user typed or pasted ("482 913", "482-913"). */
export const digitsOf = (code: string) => code.replace(/\D/g, "");

/** "482913" → "482 913" (a code already grouped is returned as it is). */
export const groupCode = (code: string) => {
  const d = digitsOf(code);
  return d.length === 6 ? `${d.slice(0, 3)} ${d.slice(3)}` : code;
};

export interface PairInput {
  address: string;
  code: string;
  /** SHA-256 of the other device's certificate: required for an `https://` address. */
  fingerprint?: string;
}

export const isHttps = (address: string) => address.trim().toLowerCase().startsWith("https://");

export type PairErrors = Partial<Record<keyof PairInput, string>>;

export function validatePair(input: PairInput): PairErrors {
  const e: PairErrors = {};
  if (!input.address.trim()) e.address = "Indica l'indirizzo del dispositivo (host:porta)";
  if (digitsOf(input.code).length !== 6) e.code = "Il codice è di 6 cifre";
  if (isHttps(input.address) && !/^[0-9a-f]{2}([: -]?[0-9a-f]{2}){31}$/i.test((input.fingerprint ?? "").trim())) {
    e.fingerprint = "Serve l'impronta del certificato (64 cifre esadecimali), che trovi in Sicurezza → Rete sull'altro dispositivo";
  }
  return e;
}

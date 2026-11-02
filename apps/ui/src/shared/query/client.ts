import { MutationCache, QueryClient } from "@tanstack/react-query";
import { errorMessage, isUnauthorized } from "../lib/errors";
import { toast } from "../lib/toast";

/** Defaults of docs/design/UI-ARCHITECTURE.md §2. */
export const QUERY_DEFAULTS = {
  staleTime: 5_000,
  refetchOnWindowFocus: true,
  refetchIntervalInBackground: false,
} as const;

/** Retry once on network or server errors, never on 401 (the pairing screen handles it). */
export function shouldRetry(failureCount: number, error: unknown): boolean {
  if (isUnauthorized(error)) return false;
  return failureCount < 1;
}

/**
 * One QueryClient for the app. A mutation without its own `onError` (and not flagged
 * `meta: { handled: true }`) shows the failure as a toast, so views never repeat
 * `try { … } catch → toast`.
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { ...QUERY_DEFAULTS, retry: shouldRetry },
      mutations: { retry: false },
    },
    mutationCache: new MutationCache({
      onError: (error, _variables, _context, mutation) => {
        if (mutation.options.onError || mutation.meta?.handled) return;
        toast(errorMessage(error), "crit");
      },
    }),
  });
}

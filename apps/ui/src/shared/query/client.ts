import { MutationCache, QueryClient } from "@tanstack/react-query";
import { errorMessage, errorStatus } from "../lib/errors";
import { toast } from "../lib/toast";

/** Defaults of docs/design/UI-ARCHITECTURE.md §2. */
export const QUERY_DEFAULTS = {
  staleTime: 5_000,
  refetchOnWindowFocus: true,
  refetchIntervalInBackground: false,
} as const;

/**
 * Retry once on network or server errors, never on a 4xx: asking again gets the same answer (a 401 is for the
 * pairing screen, a 404 means the thing is gone, a 400 means the request itself is wrong).
 */
export function shouldRetry(failureCount: number, error: unknown): boolean {
  const status = errorStatus(error);
  if (status !== undefined && status >= 400 && status < 500) return false;
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

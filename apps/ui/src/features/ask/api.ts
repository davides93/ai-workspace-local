import { useQuery } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { isNotFound } from "../../shared/lib/errors";
import { qk } from "../../shared/query/keys";

const ASK_POLL_MS = 2_500;

/**
 * Questions of the agent waiting for the person. A 404 means this backend does not have the route (yet): nothing waits, and the
 * poll goes on quietly instead of failing every few seconds.
 */
export function useQuestions() {
  const api = useApi();
  return useQuery({
    queryKey: qk.questions(),
    queryFn: () => api.questions().catch((e: unknown) => (isNotFound(e) ? [] : Promise.reject(e))),
    refetchInterval: ASK_POLL_MS,
  });
}

/** Requests for a secret waiting for the person: the list never carries a value. */
export function useSecretRequests() {
  const api = useApi();
  return useQuery({
    queryKey: qk.secretRequests(),
    queryFn: () => api.secretRequests().catch((e: unknown) => (isNotFound(e) ? [] : Promise.reject(e))),
    refetchInterval: ASK_POLL_MS,
  });
}

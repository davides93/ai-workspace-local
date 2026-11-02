import { useQuery } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { qk } from "../../shared/query/keys";

const AUDIT_POLL_MS = 5_000;

export function useAudit() {
  const api = useApi();
  return useQuery({ queryKey: qk.audit(), queryFn: () => api.audit(), refetchInterval: AUDIT_POLL_MS });
}

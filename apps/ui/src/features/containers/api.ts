import { useQuery } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { qk } from "../../shared/query/keys";
import { pollInterval } from "./model";

/** Docker/Podman of the computer. Asked again by itself until it is ready (the person is installing or starting it). */
export function useContainers({ poll = true }: { poll?: boolean } = {}) {
  const api = useApi();
  return useQuery({
    queryKey: qk.containers(),
    queryFn: () => api.containers(),
    refetchInterval: poll ? (q) => pollInterval(q.state.data) : false,
  });
}

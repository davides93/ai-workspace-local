import { useQuery } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { qk } from "../../shared/query/keys";

/**
 * LAN address of this device (`null` while the server only listens on this computer). Shares the
 * `mesh` query with the devices page, so opening both costs one request.
 */
export function useLanAddress() {
  const api = useApi();
  return useQuery({ queryKey: qk.mesh(), queryFn: () => api.mesh(), select: (m) => m.address });
}

import { useState, type ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { ApiProvider } from "../shared/api/context";
import type { Transport } from "../shared/api/transport";
import { createQueryClient } from "../shared/query/client";

/**
 * Everything the tree needs from the outside: the TanStack Query client and the transport.
 * Toasts, theme and the kill switch are small stores/hooks in `shared/` and need no provider.
 */
export function Providers({ api, children }: { api: Transport; children: ReactNode }) {
  const [client] = useState(createQueryClient);
  return (
    <QueryClientProvider client={client}>
      <ApiProvider api={api}>{children}</ApiProvider>
    </QueryClientProvider>
  );
}

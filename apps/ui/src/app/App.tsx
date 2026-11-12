import { Suspense, useEffect, useRef, useState } from "react";
import { PairingScreen } from "../features/pairing/PairingScreen";
import { ApprovalsCenter } from "../features/approvals/ApprovalsCenter";
import { McpElicitationCenter } from "../features/mcp/McpElicitationCenter";
import { useDownloadsWatcher } from "../features/models/api";
import { rememberChatSession } from "../shared/lib/navMemory";
import { useHotkey } from "../shared/lib/useHotkey";
import { useConnection } from "../shared/query/system";
import { EmptyState, PageSkeleton, ToastHost } from "../shared/ui";
import { CommandPalette } from "./palette/CommandPalette";
import { onOpenPalette } from "./palette/openPalette";
import { useCommands } from "./palette/useCommands";
import { routeLabel } from "./navigation";
import { useUiMode } from "./useUiMode";
import { RouteProvider, Link, useRoute } from "./router";
import { ROUTES } from "./routes";
import { ConnectionBanner, DemoBanner } from "./shell/Banners";
import { ErrorBoundary } from "./shell/ErrorBoundary";
import { MoreSheet } from "./shell/MoreSheet";
import { Rail } from "./shell/Rail";
import { TabBar } from "./shell/TabBar";

export function App() {
  const resolved = useRoute(ROUTES);
  const { needsPairing } = useConnection();
  const [mode] = useUiMode();
  const commands = useCommands();
  const [moreOpen, setMoreOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const mainRef = useRef<HTMLDivElement>(null);
  const current = resolved?.route.id;
  // Remember the open chat before the rail renders, so its Workspace link always points at it.
  if (resolved?.route.id === "chat" && resolved.params.sessionId) rememberChatSession(resolved.params.sessionId);
  useDownloadsWatcher();

  useHotkey({ key: "k", mod: true }, (e) => {
    e.preventDefault();
    setMoreOpen(false);
    setPaletteOpen((o) => !o);
  });

  // Anything in the app can ask for the palette (`openPalette()`), e.g. the tile of the Panoramica.
  useEffect(
    () => onOpenPalette(() => {
      setMoreOpen(false);
      setPaletteOpen(true);
    }),
    [],
  );

  // The sheet is about the section you were in: close it whenever the section changes.
  useEffect(() => setMoreOpen(false), [resolved?.path]);

  // Announce the page to assistive tech and in the tab title.
  useEffect(() => {
    document.title = resolved ? `${routeLabel(resolved.route, mode)} · AI Workspace` : "Pagina non trovata · AI Workspace";
  }, [resolved, mode]);

  const View = resolved?.route.View;
  return (
    <div className="shell">
      <button type="button" className="skip-link" onClick={() => mainRef.current?.focus()}>Vai al contenuto</button>
      <Rail current={current} onOpenPalette={() => setPaletteOpen(true)} />

      <div className="main" ref={mainRef} tabIndex={-1} id="main">
        <DemoBanner />
        <ConnectionBanner />
        <ApprovalsCenter />
        <McpElicitationCenter />
        <ErrorBoundary key={resolved?.route.id ?? "404"}>
          <Suspense fallback={<PageSkeleton />}>
            {needsPairing ? (
              <PairingScreen />
            ) : View && resolved ? (
              <RouteProvider value={{ params: resolved.params, query: resolved.query }}>
                <View />
              </RouteProvider>
            ) : (
              <div className="page">
                <EmptyState icon="search" title="Pagina non trovata" description="Questo indirizzo non corrisponde a nessuna sezione." action={<Link to="/" className="btn primary">Vai alla Panoramica</Link>} />
              </div>
            )}
          </Suspense>
        </ErrorBoundary>
      </div>

      <TabBar current={current} moreOpen={moreOpen} onMore={() => setMoreOpen(true)} />
      {moreOpen && <MoreSheet current={current} onClose={() => setMoreOpen(false)} onOpenPalette={() => setPaletteOpen(true)} />}
      {paletteOpen && <CommandPalette commands={commands} onClose={() => setPaletteOpen(false)} />}
      <ToastHost />
    </div>
  );
}

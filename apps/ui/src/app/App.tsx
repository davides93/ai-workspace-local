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

import { t, useLocale } from "../shared/i18n";
import { useLanguage } from "./useLanguage";
export function App() {
  const resolved = useRoute(ROUTES);
  const { needsPairing } = useConnection();
  const [mode] = useUiMode();
  const locale = useLocale();
  useLanguage();
  const commands = useCommands();
  const [moreOpen, setMoreOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const mainRef = useRef<HTMLElement>(null);
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
    document.title = resolved ? t("shell.page_title", { section: routeLabel(resolved.route, mode) }) : t("shell.page_title_not_found");
  }, [resolved, mode, locale]);

  const View = resolved?.route.View;
  return (
    <div className="shell">
      <button type="button" className="skip-link" onClick={() => mainRef.current?.focus()}>{t("shell.vai_al_contenuto")}</button>
      <Rail current={current} onOpenPalette={() => setPaletteOpen(true)} />

      <main className="main" ref={mainRef} tabIndex={-1} id="main">
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
                <EmptyState icon="search" title={t("shell.pagina_non_trovata")} description={t("shell.questo_indirizzo_non_corrisponde_a")} action={<Link to="/" className="btn primary">{t("shell.vai_alla_panoramica")}</Link>} />
              </div>
            )}
          </Suspense>
        </ErrorBoundary>
      </main>

      <TabBar current={current} moreOpen={moreOpen} onMore={() => setMoreOpen(true)} />
      {moreOpen && <MoreSheet current={current} onClose={() => setMoreOpen(false)} onOpenPalette={() => setPaletteOpen(true)} />}
      {paletteOpen && <CommandPalette commands={commands} onClose={() => setPaletteOpen(false)} />}
      <ToastHost />
    </div>
  );
}

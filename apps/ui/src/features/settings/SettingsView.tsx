import { useState } from "react";
import { useUiMode } from "../../app/useUiMode";
import { useTheme } from "../../shared/lib/theme";
import { UI_MODE_LABEL, UI_MODES } from "../../shared/lib/uiMode";
import { Button, EmptyState, Field, Icon, Input, Page, PageHeader, QueryState, Section, Segmented } from "../../shared/ui";
import { useSystem } from "../../shared/query/system";
import { useBackgroundActions, useConfig, useSetConfig } from "./api";
import { BackgroundBlock } from "./components/BackgroundBlock";
import { LayerBar } from "./components/LayerBar";
import { MailAccountsEditor } from "../channels/components/MailAccountsEditor";
import { UpdatePanel } from "./components/UpdatePanel";
import { SettingRow } from "./components/SettingRow";
import { changesSchedule, isPhoneOs, shouldAskNotifications } from "./model/background";
import { configTheme, DEVELOPER_ONLY, groupSettings, isTheme, LAYER_LABEL, THEME_LABEL, THEMES, winningLayer, type WritableLayer } from "./model/config";
import "./settings.css";

/** `#/settings` — the layered configuration (default → global → device → workspace) with the winning layer of every value. */
export function SettingsView() {
  const cfg = useConfig();
  const setConfig = useSetConfig();
  const [theme, setTheme] = useTheme();
  const [uiMode, setUiMode] = useUiMode();
  const [layer, setLayer] = useState<WritableLayer>("global");
  const [query, setQuery] = useState("");
  const system = useSystem();
  const bg = useBackgroundActions();
  const os = system.data?.device.os;

  const commit = (key: string, title: string, value: unknown) => {
    const before = cfg.data?.effective["proactive.level"];
    setConfig.mutate(
      { layer, key, value, title },
      {
        onSuccess: () => {
          if (!isPhoneOs(os) || !changesSchedule(key)) return;
          // The phone is told again what to schedule; the notification permission is asked only when the assistant is turned on.
          if (key === "proactive.level" && shouldAskNotifications(before, value)) bg.askNotifications.mutate(undefined, { onSettled: () => bg.configure.mutate() });
          else bg.configure.mutate();
        },
      },
    );
    // The theme is applied here as well: the choice shows at once, whatever the layer says.
    if (key === "ui.theme" && isTheme(value)) setTheme(value);
  };
  const chooseTheme = (t: typeof theme) => {
    setTheme(t);
    commit("ui.theme", "Tema", t);
  };
  const wanted = configTheme(cfg.data);

  return (
    <Page>
      <PageHeader
        eyebrow="Controllo" title="Impostazioni"
        subtitle="Configurazione a strati: default → globale → device → workspace. Per ogni valore vedi quale strato vince."
        actions={<Segmented label="Tema" value={theme} onChange={chooseTheme} options={THEMES.map((t) => ({ id: t, label: THEME_LABEL[t] }))} />}
      />
      {cfg.data && wanted && wanted !== theme && (
        <p className="set-note text-sm">
          <Icon name="info" size={15} />
          <span>
            La configurazione ({LAYER_LABEL[winningLayer(cfg.data, "ui.theme")]}) indica il tema «{THEME_LABEL[wanted]}», ma questo dispositivo usa «{THEME_LABEL[theme]}».
          </span>
          <Button size="sm" onClick={() => setTheme(wanted)}>Usa la configurazione</Button>
        </p>
      )}

      <div className="panel set-mode">
        <div className="set-mode-text">
          <b>Modalità dell'interfaccia</b>
          <span className="faint text-sm">
            {uiMode === "simple"
              ? "Semplice mostra solo Panoramica, Chat, Assistenti, Modelli e Impostazioni. Le altre sezioni (runtime, profili, sicurezza, valutazioni, addestramento…) si vedono nella vista Sviluppatore."
              : "La vista Sviluppatore mostra tutte le sezioni: runtime, profili, sicurezza, valutazioni, addestramento… Semplice le riduce a Panoramica, Chat, Assistenti, Modelli e Impostazioni."}
          </span>
        </div>
        <Segmented label="Modalità dell'interfaccia" value={uiMode} onChange={setUiMode} options={UI_MODES.map((m) => ({ id: m, label: UI_MODE_LABEL[m] }))} />
      </div>

      <UpdatePanel />

      <QueryState query={cfg}>
        {(view) => {
          const groups = groupSettings(view.schema, query)
            .map((group) => ({ ...group, entries: group.entries.filter((entry) => entry.key !== "update.channel" && entry.key !== "connectors.mail.imap" && (uiMode === "developer" || !DEVELOPER_ONLY.includes(entry.key))) }))
            .filter((group) => group.entries.length > 0);
          return (
            <>
              <LayerBar view={view} layer={layer} onLayer={setLayer} />
              <Field label="Cerca un'impostazione" htmlFor="set-search" className="set-search">
                <Input id="set-search" type="search" placeholder="Nome, chiave o descrizione" value={query} onChange={(e) => setQuery(e.target.value)} />
              </Field>
              {groups.length === 0 && (
                <EmptyState icon="search" title="Nessuna impostazione corrisponde" description="Prova con un altro termine." action={<Button onClick={() => setQuery("")}>Cancella la ricerca</Button>} />
              )}
              {groups.map(({ section, entries }) => (
                <Section key={section.id} title={section.title} hint={section.description}>
                  <div className="panel">
                    <ul className="set-list">
                      {entries.map((e) => <SettingRow key={e.key} view={view} entry={e} layer={layer} onCommit={commit} />)}
                    </ul>
                  </div>
                  {section.id === "connectors" && <MailAccountsEditor />}
                  {section.id === "proactive" && <BackgroundBlock os={os} level={view.effective["proactive.level"] as never} />}
                </Section>
              ))}
            </>
          );
        }}
      </QueryState>
    </Page>
  );
}

import { useState } from "react";
import { LanguageSelect } from "../../app/LanguageSelect";
import { t, t as tr } from "../../shared/i18n";
import { useUiMode } from "../../app/useUiMode";
import { useTheme } from "../../shared/lib/theme";
import { UI_MODE_LABEL, UI_MODES } from "../../shared/lib/uiMode";
import { VoiceBlock } from "./components/VoiceBlock";
import { Button, EmptyState, Field, Icon, Input, Page, PageHeader, QueryState, Section, Segmented } from "../../shared/ui";
import { useSystem } from "../../shared/query/system";
import { useBackgroundActions, useConfig, useSetConfig } from "./api";
import { BackgroundBlock } from "./components/BackgroundBlock";
import { LayerBar } from "./components/LayerBar";
import { MailAccountsEditor } from "../channels/components/MailAccountsEditor";
import { BackupPanel } from "./components/BackupPanel";
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
        eyebrow="Controllo" title={tr("settings.impostazioni")}
        subtitle={tr("settings.configurazione_a_strati_default_globale")}
        actions={<Segmented label={tr("settings.tema")} value={theme} onChange={chooseTheme} options={THEMES.map((t) => ({ id: t, label: THEME_LABEL[t] }))} />}
      />
      {cfg.data && wanted && wanted !== theme && (
        <p className="set-note text-sm">
          <Icon name="info" size={15} />
          <span>
            {tr("settings.la_configurazione_indica_il_tema", { p: LAYER_LABEL[winningLayer(cfg.data, "ui.theme")], p2: THEME_LABEL[wanted], p3: THEME_LABEL[theme] })}</span>
          <Button size="sm" onClick={() => setTheme(wanted)}>{tr("settings.usa_la_configurazione")}</Button>
        </p>
      )}

      <div className="panel set-mode">
        <div className="set-mode-text">
          <b>{tr("settings.modalita_dell_interfaccia")}</b>
          <span className="faint text-sm">
            {uiMode === "simple" ? tr("settings.semplice_mostra_solo_panoramica_chat") : tr("settings.la_vista_sviluppatore_mostra_tutte")}
          </span>
        </div>
        <Segmented label={tr("settings.modalita_dell_interfaccia")} value={uiMode} onChange={setUiMode} options={UI_MODES.map((m) => ({ id: m, label: UI_MODE_LABEL[m] }))} />
      </div>

      <div className="panel set-mode">
        <div className="set-mode-text">
          <b>{t("common.language")}</b>
          <span className="faint text-sm">{t("common.language_hint")}</span>
        </div>
        <LanguageSelect />
      </div>

      <UpdatePanel />
      <BackupPanel />

      <QueryState query={cfg}>
        {(view) => {
          const groups = groupSettings(view.schema, query)
            .map((group) => ({ ...group, entries: group.entries.filter((entry) => entry.key !== "update.channel" && entry.key !== "voice.tts.voice" && entry.key !== "connectors.mail.imap" && (uiMode === "developer" || !DEVELOPER_ONLY.includes(entry.key))) }))
            .filter((group) => group.entries.length > 0);
          return (
            <>
              <LayerBar view={view} layer={layer} onLayer={setLayer} />
              <Field label={tr("settings.cerca_un_impostazione")} htmlFor="set-search" className="set-search">
                <Input id="set-search" type="search" placeholder={tr("settings.nome_chiave_o_descrizione")} value={query} onChange={(e) => setQuery(e.target.value)} />
              </Field>
              {groups.length === 0 && (
                <EmptyState icon="search" title={tr("settings.nessuna_impostazione_corrisponde")} description={tr("settings.prova_con_un_altro_termine")} action={<Button onClick={() => setQuery("")}>{tr("settings.cancella_la_ricerca")}</Button>} />
              )}
              {groups.map(({ section, entries }) => (
                <Section key={section.id} title={section.title} hint={section.description}>
                  <div className="panel">
                    <ul className="set-list">
                      {entries.map((e) => <SettingRow key={e.key} view={view} entry={e} layer={layer} onCommit={commit} />)}
                    </ul>
                  </div>
                  {section.id === "connectors" && <MailAccountsEditor />}
                  {section.id === "voice" && <VoiceBlock picks={view.effective["voice.tts.voice"]} onPick={(v) => commit("voice.tts.voice", "Voce neurale", v)} />}
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

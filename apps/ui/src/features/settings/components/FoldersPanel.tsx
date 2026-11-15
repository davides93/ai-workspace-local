import { useApi } from "../../../shared/api/context";
import { t } from "../../../shared/i18n";
import { Button, Pill, Section } from "../../../shared/ui";
import { useFolderActions, useFolders } from "../api";

/**
 * «Cartelle consentite» (Mac App Store build): the folders of the Mac the person lets the agents use besides the app's own
 * folder. They are added with the macOS panel and kept as a permission that lasts across restarts; only the native app can
 * add or take off one, because no route of the API does (a model cannot widen its own access).
 */
export function FoldersPanel({ workspace }: { workspace: string }) {
  const api = useApi();
  const folders = useFolders(true);
  const actions = useFolderActions();
  const canEdit = api.addFolders !== undefined && api.removeFolder !== undefined;
  const list = folders.data?.folders ?? [];
  const refused = actions.add.data?.refused ?? [];

  return (
    <Section title={t("settings.cartelle_consentite_titolo")} hint={t("settings.cartelle_consentite_hint")}>
    <div className="panel set-folders" data-testid="folders-panel">
      <p className="set-hint">{t("settings.cartelle_spiegazione", { workspace })}</p>
      {list.length === 0 ? (
        <p className="set-hint" data-testid="folders-empty">{t("settings.cartelle_vuote")}</p>
      ) : (
        <ul className="set-folder-list" aria-label={t("settings.cartelle_consentite_titolo")}>
          {list.map((f) => (
            <li key={f.id} className="set-folder">
              <div className="set-folder-main">
                <span className="set-folder-name">
                  <b>{f.name}</b>
                  {!f.available && <Pill tone="warn">{t("settings.cartelle_non_disponibile")}</Pill>}
                </span>
                {f.available ? <span className="mono faint text-sm set-folder-path">{f.path}</span> : <span className="faint text-sm">{f.problem ?? t("settings.cartelle_non_disponibile")}</span>}
              </div>
              {canEdit && (
                <Button size="sm" aria-label={t("settings.cartelle_togli_nome", { name: f.name })} disabled={actions.remove.isPending} onClick={() => actions.remove.mutate({ id: f.id, name: f.name })}>
                  {t("settings.cartelle_togli")}
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
      {refused.length > 0 && (
        <div role="status" className="set-warn">
          <span>
            <b>{t("settings.cartelle_rifiutate")}</b>
            {refused.map((r) => <span key={r} className="set-folder-refused">{r}</span>)}
          </span>
        </div>
      )}
      {canEdit ? (
        <div className="set-bg-actions">
          <Button variant="primary" icon="plus" disabled={actions.add.isPending} onClick={() => actions.add.mutate()}>{t("settings.cartelle_aggiungi")}</Button>
        </div>
      ) : (
        <p className="set-hint">{t("settings.cartelle_solo_app")}</p>
      )}
      <p className="set-hint">{t("settings.cartelle_nota_file")}</p>
    </div>
    </Section>
  );
}

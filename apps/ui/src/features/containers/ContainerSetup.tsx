import { Button, Icon, Sheet, SkeletonLines } from "../../shared/ui";
import { errorMessage } from "../../shared/lib/errors";
import { t } from "../../shared/i18n";
import { useContainers } from "./api";
import { setupView } from "./model";

/**
 * «Un computer per i tuoi agenti»: the guided check of Docker, in the simple interface's menu. It only looks and explains
 * (installed? running? with compose?) and gives the one next step; it installs and starts nothing. When everything is ready it
 * says so and stops asking. The next step (preparing the agents' computer) comes with its own button when it exists.
 */
export function ContainerSetup({ onClose }: { onClose: () => void }) {
  const q = useContainers();
  const view = q.data ? setupView(q.data) : undefined;
  return (
    <Sheet title={t("containers.titolo")} onClose={onClose}>
      <div className="stack stack-lg">
        <p className="muted">{t("containers.intro")}</p>
        {q.isError && (
          <div role="alert" className="stack stack-sm">
            <p>{t("containers.errore", { error: errorMessage(q.error) })}</p>
            <Button onClick={() => void q.refetch()}>{t("containers.riprova")}</Button>
          </div>
        )}
        {q.isPending && <SkeletonLines lines={3} />}
        {view && (
          <div className="stack stack-sm" role="status" aria-live="polite">
            <h3 className="row" style={{ gap: 8 }}>
              <Icon name={view.tone === "ok" ? "check" : view.tone === "info" ? "device" : "chip"} />
              <span>{t(view.title, view.params)}</span>
            </h3>
            <p>{t(view.body, view.params)}</p>
            {view.link && (
              <p>
                <a href={view.link.href} target="_blank" rel="noreferrer">{t(view.link.label)}</a>
              </p>
            )}
            {view.tone === "todo" && <p className="muted text-sm">{t("containers.controllo_automatico")}</p>}
            {view.tone !== "info" && (
              <div className="row">
                <Button disabled={q.isFetching} onClick={() => void q.refetch()}>{t("containers.controlla_ancora")}</Button>
              </div>
            )}
          </div>
        )}
      </div>
    </Sheet>
  );
}

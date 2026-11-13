import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useApi } from "../../../shared/api/context";
import { errorMessage } from "../../../shared/lib/errors";
import { downloadBase64, downloadFile } from "../../../shared/lib/download";
import { formatBytes } from "../../../shared/lib/format";
import { toast } from "../../../shared/lib/toast";
import { t } from "../../../shared/i18n";
import { Button, Pill, Section } from "../../../shared/ui";

/** Backup of the data folder and the exportable diagnostics. Restore is CLI-only (daemon stopped) and the panel says so. */
export function BackupPanel() {
  const api = useApi();
  const [withSecrets, setWithSecrets] = useState(false);
  const backup = useMutation({
    mutationFn: () => api.createBackup(withSecrets),
    onSuccess: (f) => {
      downloadBase64(f.file_name, f.data, "application/gzip");
      toast(t("settings.backup_creato", { p: `${f.file_name} (${formatBytes(f.size)})` }));
    },
  });
  const diag = useMutation({
    mutationFn: () => api.diagnostics(),
    onSuccess: (d) => {
      downloadFile("aiw-diagnostics.json", JSON.stringify(d, null, 2));
      toast(t("settings.diagnostica_scaricata"));
    },
  });
  const error = (backup.error && errorMessage(backup.error)) || (diag.error && errorMessage(diag.error)) || null;

  return (
    <Section title={t("settings.backup_titolo")} hint={t("settings.backup_hint")}>
      <div className="panel bkp stack">
        <p className="set-hint">{t("settings.backup_cosa_contiene")}</p>
        <p className="set-hint">{t("settings.backup_cosa_esclude")}</p>
        <label className="bkp-check">
          <input type="checkbox" checked={withSecrets} onChange={(e) => setWithSecrets(e.target.checked)} />
          <span>{t("settings.backup_includi_segreti")}</span>
          {withSecrets && <Pill tone="warn">{t("settings.backup_sensibile")}</Pill>}
        </label>
        {withSecrets && <p className="set-warn"><span>{t("settings.backup_segreti_avviso")}</span></p>}
        <div className="bkp-actions">
          <Button variant="primary" disabled={backup.isPending} onClick={() => backup.mutate()}>{t("settings.backup_crea")}</Button>
        </div>
        <p className="set-hint"><b>{t("settings.backup_ripristino_titolo")}.</b> {t("settings.backup_ripristino_testo")}</p>
        <p className="set-hint">{t("settings.diagnostica_descrizione")}</p>
        <div className="bkp-actions">
          <Button disabled={diag.isPending} onClick={() => diag.mutate()}>{t("settings.diagnostica_scarica")}</Button>
        </div>
        {error && <p role="alert" className="set-warn"><span>{error}</span></p>}
      </div>
    </Section>
  );
}

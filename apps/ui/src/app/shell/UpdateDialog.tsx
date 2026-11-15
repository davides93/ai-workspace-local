import { lazy, Suspense } from "react";
import { Dialog, SkeletonLines } from "../../shared/ui";

import { t } from "../../shared/i18n";
// The updater panel lives with the settings; load it only when someone opens the notice.
const UpdatePanel = lazy(() => import("../../features/settings/components/UpdatePanel").then((m) => ({ default: m.UpdatePanel })));

/** The notice of a new version in the simple interface: the updater itself in a dialog, since the Impostazioni page is not there. */
export function UpdateDialog({ onClose }: { onClose: () => void }) {
  return (
    <Dialog title={t("shell.aggiornamento_disponibile_titolo")} onClose={onClose} size="lg">
      <Suspense fallback={<SkeletonLines lines={4} />}>
        <UpdatePanel />
      </Suspense>
    </Dialog>
  );
}

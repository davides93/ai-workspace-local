import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../../shared/api/context";
import type { DatasetInfo } from "../../../shared/api/types";
import { errorMessage } from "../../../shared/lib/errors";
import { qk } from "../../../shared/query/keys";
import { Button, Field, Input } from "../../../shared/ui";

import { t, tNode } from "../../../shared/i18n";
export function ImportSftExamples({ onImported }: { onImported: (id: string) => void }) {
  const [file, setFile] = useState<File | null>(null);
  const api = useApi();
  const qc = useQueryClient();
  const upload = useMutation({
    meta: { handled: true },
    mutationFn: async (selected: File) => {
      if (selected.size > 1_000_000) throw new Error(t("training.il_file_di_esempi_supera"));
      return api.importSftDataset(selected.name.replace(/\.jsonl$/i, ""), await selected.text());
    },
    onSuccess: (dataset) => {
      qc.setQueryData<DatasetInfo[]>(qk.training.datasets(), (list) => [dataset, ...(list ?? [])]);
      void qc.invalidateQueries({ queryKey: qk.training.datasets() });
      onImported(dataset.id);
    },
  });
  return (
    <details>
      <summary>{t("training.importa_esempi_da_un_file")}</summary>
      <div className="stack">
        <p className="text-sm">{tNode("training.prepara_almeno_10_esempi_distinti", { user: <code>user</code>, assistant: <code>assistant</code> })}</p>
        <pre className="logbox">{`{"messages":[{"role":"user","content":${JSON.stringify(t("training.sample_context"))}},{"role":"assistant","content":${JSON.stringify(t("training.sample_answer"))}}]}`}</pre>
        <Field label={t("training.file_di_esempi_jsonl")} htmlFor="train-sft-import" hint={t("training.massimo_1_mb_il_dataset")}>
          <Input id="train-sft-import" type="file" accept=".jsonl,application/x-ndjson" disabled={upload.isPending}
            onChange={(event) => { upload.reset(); setFile(event.target.files?.[0] ?? null); }} />
        </Field>
        <Button type="button" disabled={!file || upload.isPending || upload.isSuccess}
          onClick={() => { if (file) upload.mutate(file); }}>
          {upload.isPending ? t("training.importazione") : upload.isSuccess ? t("training.dataset_importato") : t("training.importa_gli_esempi")}
        </Button>
        {upload.isError && <p className="tr-error" role="alert">{errorMessage(upload.error)}</p>}
      </div>
    </details>
  );
}

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../../shared/api/context";
import type { DatasetInfo } from "../../../shared/api/types";
import { errorMessage } from "../../../shared/lib/errors";
import { qk } from "../../../shared/query/keys";
import { Button, Field, Input } from "../../../shared/ui";

export function ImportSftExamples({ onImported }: { onImported: (id: string) => void }) {
  const [file, setFile] = useState<File | null>(null);
  const api = useApi();
  const qc = useQueryClient();
  const upload = useMutation({
    meta: { handled: true },
    mutationFn: async (selected: File) => {
      if (selected.size > 1_000_000) throw new Error("Il file di esempi supera 1 MB.");
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
      <summary>Importa esempi da un file locale</summary>
      <div className="stack">
        <p className="text-sm">Prepara almeno 10 esempi distinti. In ogni riga, metti in <code>user</code> il contesto e la domanda e in <code>assistant</code> la risposta che vuoi insegnare. Rimuovi i dettagli personali che non servono.</p>
        <pre className="logbox">{'{"messages":[{"role":"user","content":"Contesto e domanda"},{"role":"assistant","content":"Risposta di esempio"}]}'}</pre>
        <Field label="File di esempi JSONL" htmlFor="train-sft-import" hint="Massimo 1 MB. Il dataset viene salvato sul dispositivo che esegue AIW.">
          <Input id="train-sft-import" type="file" accept=".jsonl,application/x-ndjson" disabled={upload.isPending}
            onChange={(event) => { upload.reset(); setFile(event.target.files?.[0] ?? null); }} />
        </Field>
        <Button type="button" disabled={!file || upload.isPending || upload.isSuccess}
          onClick={() => { if (file) upload.mutate(file); }}>
          {upload.isPending ? "Importazione…" : upload.isSuccess ? "Dataset importato" : "Importa gli esempi"}
        </Button>
        {upload.isError && <p className="tr-error" role="alert">{errorMessage(upload.error)}</p>}
      </div>
    </details>
  );
}

import { useEffect, useState, type FormEvent } from "react";
import { useQueryParams } from "../../../app/router";
import { toast } from "../../../shared/lib/toast";
import { Button, Field, Icon, Input, Panel } from "../../../shared/ui";
import { usePullModel } from "../api";
import { buildUrlRequest, validateUrlForm, type UrlFormValues } from "../model/forms";
import { InlineError } from "./HubError";

const EMPTY: UrlFormValues = { url: "", fileName: "", sha256: "" };

/** Direct download of one file from any HTTP(S) address, with an optional name and sha256 check. */
export function UrlPullForm() {
  const prefill = useQueryParams().url ?? "";
  const [values, setValues] = useState<UrlFormValues>({ ...EMPTY, url: prefill });
  const [submitted, setSubmitted] = useState(false);
  const pull = usePullModel({ handled: true });

  // "Riprova" of a failed URL job reopens the form with its address filled in.
  useEffect(() => {
    if (prefill) setValues((v) => (v.url === prefill ? v : { ...v, url: prefill }));
  }, [prefill]);

  const errors = validateUrlForm(values);
  const shown = submitted ? errors : {};
  const set = (key: keyof UrlFormValues) => (e: { target: { value: string } }) => setValues((v) => ({ ...v, [key]: e.target.value }));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (Object.keys(errors).length > 0) return;
    pull.mutate(buildUrlRequest(values), {
      onSuccess: () => {
        toast("Download avviato: lo segui qui sopra o nella scheda Download");
        setValues(EMPTY);
        setSubmitted(false);
      },
    });
  };

  return (
    <Panel title="Scarica da un indirizzo" icon="link">
      <form className="form-grid mh-url-form" onSubmit={submit} noValidate>
        <Field label="Indirizzo del file" htmlFor="url-address" error={shown.url} hint="Per esempio il link «resolve» di un file .gguf, .onnx o .safetensors.">
          <Input
            id="url-address" type="url" mono inputMode="url" autoComplete="off" spellCheck={false}
            placeholder="https://esempio.org/modelli/qwen3-4b-Q4_K_M.gguf" value={values.url} onChange={set("url")}
            aria-invalid={shown.url ? true : undefined}
          />
        </Field>
        <div className="mh-url-pair">
          <Field label="Nome del file (facoltativo)" htmlFor="url-name" error={shown.fileName} hint="Se vuoto si usa il nome presente nell'indirizzo.">
            <Input id="url-name" mono autoComplete="off" spellCheck={false} placeholder="modello.gguf" value={values.fileName} onChange={set("fileName")} aria-invalid={shown.fileName ? true : undefined} />
          </Field>
          <Field label="sha256 (facoltativo)" htmlFor="url-sha" error={shown.sha256} hint="Se lo indichi, a fine download il file viene verificato e scartato se non coincide.">
            <Input id="url-sha" mono autoComplete="off" spellCheck={false} placeholder="64 caratteri esadecimali" value={values.sha256} onChange={set("sha256")} aria-invalid={shown.sha256 ? true : undefined} />
          </Field>
        </div>
        <p className="faint text-sm mh-trust">
          <Icon name="shield" size={14} /> Nessun token viene inviato a questo indirizzo. Scarica solo da siti di cui ti fidi: un modello è un file binario.
        </p>
        {pull.isError && <InlineError error={pull.error} />}
        <div className="row">
          <Button type="submit" variant="primary" icon="download" disabled={pull.isPending}>{pull.isPending ? "Avvio…" : "Scarica"}</Button>
        </div>
      </form>
    </Panel>
  );
}

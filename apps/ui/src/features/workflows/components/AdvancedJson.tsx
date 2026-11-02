import { useState } from "react";
import { Button, Icon, Textarea } from "../../../shared/ui";
import { parseWorkflowJson, previewJson, type PlainDraft } from "../model/draft";

/** The whole definition as JSON, for what the form cannot say. Applying it replaces the form; saving is a separate step. */
export function AdvancedJson({ plain, workflowId, onApply }: { plain: PlainDraft; workflowId: string; onApply: (d: PlainDraft) => void }) {
  const current = previewJson(plain);
  const [text, setText] = useState(current);
  const [error, setError] = useState<string | null>(null);
  const [applied, setApplied] = useState(false);
  const edited = text !== current;

  const apply = () => {
    const r = parseWorkflowJson(text, workflowId);
    if (!r.ok) {
      setError(r.error);
      setApplied(false);
      return;
    }
    setError(null);
    setApplied(true);
    onApply({ ...r.draft, id: plain.id });
    setText(previewJson({ ...r.draft, id: plain.id }));
  };

  return (
    <div className="stack">
      <p className="muted text-sm">Modifica la definizione completa. «Applica al modulo» la controlla e aggiorna il modulo: poi salvi come sempre.</p>
      <Textarea
        className="json-editor mono" aria-label="Definizione del workflow (JSON)" rows={22} spellCheck={false} value={text}
        onChange={(e) => { setText(e.target.value); setApplied(false); setError(null); }} aria-invalid={!!error} aria-describedby={error ? "adv-error" : undefined}
      />
      {error && <p id="adv-error" className="field-error row" role="alert"><Icon name="alert" size={14} /> {error}</p>}
      {applied && !error && <p className="row text-sm" role="status"><Icon name="check" size={14} /> Applicato: controlla il modulo e salva.</p>}
      <div className="row">
        <Button variant="primary" onClick={apply} disabled={!edited}>Applica al modulo</Button>
        <Button variant="ghost" onClick={() => { setText(current); setError(null); setApplied(false); }} disabled={!edited}>Ripristina dal modulo</Button>
      </div>
      <p className="faint text-xs">
        Il segreto di un webhook è nascosto e non si cambia da qui: lo genera il backend. Il backend legge anche file YAML in <span className="mono">.aiw/workflows/</span>.
      </p>
    </div>
  );
}

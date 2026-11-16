import { useEffect, useId, useRef, useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { toast } from "../../../shared/lib/toast";
import { formatBytes } from "../../../shared/lib/format";
import type { ChannelInfo, EmailDraft } from "../../../shared/api/types";
import { Button, Field, IconButton, Input, Select, TagInput, Textarea } from "../../../shared/ui";
import { useCreateDraft, useUpdateDraft } from "../api";

import { t } from "../../../shared/i18n";

const same = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i]);

/**
 * Writes a new draft (no `draft`) or changes one that is waiting. Attachments are paths inside the workspace: the file is
 * copied when the draft is saved, so what is shown afterwards is what goes. A saved change is a new revision with a new
 * fingerprint: the approval is always of what is on screen after saving.
 */
export function DraftEditor({ draft, channels, onDone }: { draft?: EmailDraft; channels: ChannelInfo[]; onDone: () => void }) {
  const create = useCreateDraft();
  const update = useUpdateDraft();
  const [channelId, setChannelId] = useState(draft?.channel_id ?? channels[0]?.id ?? "");
  const [to, setTo] = useState(draft?.to ?? []);
  const [cc, setCc] = useState(draft?.cc ?? []);
  const [bcc, setBcc] = useState(draft?.bcc ?? []);
  const [subject, setSubject] = useState(draft?.subject ?? "");
  const [body, setBody] = useState(draft?.body ?? "");
  const [removed, setRemoved] = useState<string[]>([]);
  const [added, setAdded] = useState<string[]>([]);
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  // The revision the editor opened on: a save over a draft that changed since (the assistant revised it) is refused, not merged.
  const [openedOn] = useState(draft?.revision);
  const form = useRef<HTMLFormElement>(null);
  const uid = useId();
  useEffect(() => {
    form.current?.querySelector<HTMLElement>("input, textarea, select")?.focus();
  }, []);
  const kept = (draft?.attachments ?? []).filter((a) => !removed.includes(a.sha256));
  const busy = create.isPending || update.isPending;

  const addPath = () => {
    const p = path.trim();
    if (p && !added.includes(p)) setAdded([...added, p]);
    setPath("");
  };
  const save = () => {
    setError(null);
    const onError = (e: unknown) => setError(errorMessage(e));
    const done = () => { toast(t("channels.bozza_salvata")); onDone(); };
    if (!draft) {
      create.mutate({ channel_id: channelId || undefined, to, cc, bcc, subject, body, attachments: added }, { onSuccess: done, onError });
      return;
    }
    update.mutate({
      id: draft.id,
      patch: {
        channel_id: channelId !== draft.channel_id ? channelId : undefined,
        to: same(to, draft.to) ? undefined : to,
        cc: same(cc, draft.cc) ? undefined : cc,
        bcc: same(bcc, draft.bcc) ? undefined : bcc,
        subject: subject !== draft.subject ? subject : undefined,
        body: body !== draft.body ? body : undefined,
        add_attachments: added,
        remove_attachments: removed,
        expected_revision: openedOn,
      },
    }, { onSuccess: done, onError });
  };

  return (
    <form className="draft-edit" ref={form} onSubmit={(e) => { e.preventDefault(); save(); }} aria-label={draft ? t("channels.bozza_modifica") : t("channels.bozza_nuova")}>
      {channels.length > 1 && (
        <Field label={t("channels.bozza_canale")} htmlFor={`${uid}-ch`}>
          <Select id={`${uid}-ch`} value={channelId} onChange={(e) => setChannelId(e.target.value)}>
            {channels.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </Select>
        </Field>
      )}
      <Field label={t("channels.bozza_a")} htmlFor={`${uid}-to`} hint={t("channels.bozza_a_hint")}>
        <TagInput id={`${uid}-to`} label={t("channels.bozza_a")} value={to} onChange={setTo} unique mono={false} placeholder="nome@example.com" />
      </Field>
      <Field label={t("channels.bozza_cc")} htmlFor={`${uid}-cc`}>
        <TagInput id={`${uid}-cc`} label={t("channels.bozza_cc")} value={cc} onChange={setCc} unique mono={false} placeholder="nome@example.com" />
      </Field>
      <Field label={t("channels.bozza_ccn")} htmlFor={`${uid}-bcc`}>
        <TagInput id={`${uid}-bcc`} label={t("channels.bozza_ccn")} value={bcc} onChange={setBcc} unique mono={false} placeholder="nome@example.com" />
      </Field>
      <Field label={t("channels.bozza_oggetto")} htmlFor={`${uid}-subject`}>
        <Input id={`${uid}-subject`} value={subject} maxLength={300} onChange={(e) => setSubject(e.target.value)} />
      </Field>
      <Field label={t("channels.bozza_testo")} htmlFor={`${uid}-body`}>
        <Textarea id={`${uid}-body`} rows={10} value={body} onChange={(e) => setBody(e.target.value)} />
      </Field>
      <div className="field">
        <span className="chn-label">{t("channels.bozza_allegati")}</span>
        {kept.length === 0 && added.length === 0 && <span className="faint text-sm">{t("channels.bozza_nessun_allegato")}</span>}
        <ul className="draft-files">
          {kept.map((a) => (
            <li key={a.sha256}>
              <span className="grow"><strong>{a.name}</strong> · {formatBytes(a.size)}</span>
              <IconButton icon="trash" size={15} aria-label={t("channels.bozza_rimuovi_allegato", { name: a.name })} onClick={() => setRemoved([...removed, a.sha256])} />
            </li>
          ))}
          {added.map((p) => (
            <li key={p}>
              <span className="grow"><strong className="mono">{p}</strong></span>
              <IconButton icon="trash" size={15} aria-label={t("channels.bozza_rimuovi_allegato", { name: p })} onClick={() => setAdded(added.filter((x) => x !== p))} />
            </li>
          ))}
        </ul>
        <div className="row">
          <Input aria-label={t("channels.bozza_allegato_percorso")} aria-describedby={`${uid}-path-hint`} mono value={path} placeholder="docs/preventivo.pdf" onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); addPath(); } }} />
          <Button disabled={!path.trim()} onClick={addPath}>{t("channels.bozza_aggiungi")}</Button>
        </div>
        <span id={`${uid}-path-hint`} className="hint">{t("channels.bozza_allegato_hint")}</span>
      </div>
      {error && <p className="chn-error" role="alert">{error}</p>}
      <div className="row">
        <Button variant="primary" type="submit" disabled={busy}>{draft ? t("channels.bozza_salva") : t("channels.bozza_crea")}</Button>
        <Button variant="ghost" onClick={onDone}>{t("channels.bozza_chiudi")}</Button>
      </div>
    </form>
  );
}

import { useId, useRef, useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { toast } from "../../../shared/lib/toast";
import type { ChannelInfo, ContextInfo } from "../../../shared/api/types";
import { Button, DecisionDialog, Field, Icon, Input, Pill, Select } from "../../../shared/ui";
import { Link } from "../../../app/router";
import { useContexts, useCreateContext, useDeleteContext } from "../api";
import { MODE_LABEL, kindInfo } from "../model/channels";
import { linkableChannels, linkableSenders, pickedMembers, takenSenders } from "../model/contexts";

import { t } from "../../../shared/i18n";

/** One shared context: who is in it, why an account is out, the way to its conversation and the way to unlink it. */
function ContextCard({ ctx }: { ctx: ContextInfo }) {
  const remove = useDeleteContext();
  const [confirm, setConfirm] = useState(false);
  const title = useId();
  return (
    <article className="panel ctx" aria-labelledby={title}>
      <header className="row">
        <h3 id={title} className="grow">{ctx.name}</h3>
        <Pill tone={ctx.mode === "ask" ? "info" : "neutral"}>{MODE_LABEL[ctx.mode]}</Pill>
      </header>
      <ul className="ctx-members" aria-label={t("channels.ctx_account_elenco", { name: ctx.name })}>
        {ctx.members_info.map((m) => (
          <li key={`${m.channel_id}|${m.sender}`}>
            <span><strong>{m.channel_name || m.channel_id}</strong> · {kindInfo(m.kind).label} · <span className="mono">{m.sender}</span></span>
            {m.problem && <span className="chn-warn"><Icon name="alert" size={14} /> {t("channels.ctx_problema", { problem: m.problem })}</span>}
          </li>
        ))}
      </ul>
      <div className="row">
        <Link to={`/chat/${ctx.session_id}`} className="btn">{t("channels.ctx_apri")}</Link>
        <span className="grow" />
        <Button variant="ghost" icon="trash" onClick={() => setConfirm(true)}>{t("channels.ctx_scollega")}</Button>
      </div>
      {confirm && (
        <DecisionDialog
          title={t("channels.ctx_scollega")}
          confirmLabel={t("channels.ctx_si_scollega")}
          pending={remove.isPending}
          error={remove.error ?? undefined}
          onCancel={() => setConfirm(false)}
          onConfirm={() => remove.mutate(ctx.id, { onSuccess: () => { setConfirm(false); toast(t("channels.ctx_scollegati")); } })}
        >
          <p>{t("channels.ctx_scollega_nota")}</p>
        </DecisionDialog>
      )}
    </article>
  );
}

/** The form that links accounts: one account per channel, picked from the list of who may write there. */
function LinkForm({ channels, taken, onDone }: { channels: ChannelInfo[]; taken: Set<string>; onDone: () => void }) {
  const create = useCreateContext();
  const [name, setName] = useState("");
  const [picks, setPicks] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const uid = useId();
  const members = pickedMembers(channels, picks);
  const submit = () => {
    setError(null);
    if (members.length === 0) return setError(t("channels.ctx_scegli_due"));
    create.mutate({ name, members }, {
      onSuccess: () => { toast(t("channels.ctx_collegati")); onDone(); },
      onError: (e) => setError(errorMessage(e)),
    });
  };
  return (
    <form className="panel ctx" onSubmit={(e) => { e.preventDefault(); submit(); }} aria-label={t("channels.ctx_nuovo")}>
      <h3>{t("channels.ctx_nuovo")}</h3>
      <div className="chn-form">
        <Field label={t("channels.ctx_nome")} htmlFor={`${uid}-name`}>
          <Input id={`${uid}-name`} value={name} maxLength={60} autoFocus placeholder={t("channels.ctx_nome_esempio")} onChange={(e) => setName(e.target.value)} />
        </Field>
        {channels.map((c) => (
          <Field key={c.id} label={t("channels.ctx_account_su", { channel: c.name })} htmlFor={`${uid}-${c.id}`}>
            <Select id={`${uid}-${c.id}`} value={picks[c.id] ?? ""} onChange={(e) => setPicks({ ...picks, [c.id]: e.target.value })}>
              <option value="">{t("channels.ctx_non_collegare")}</option>
              {linkableSenders(c).map((s) => <option key={s} value={s} disabled={taken.has(`${c.id}|${s}`)}>{s}</option>)}
            </Select>
          </Field>
        ))}
      </div>
      {error && <p className="chn-error" role="alert">{error}</p>}
      <div className="row">
        <Button variant="primary" type="submit" disabled={create.isPending || !name.trim()}>{t("channels.ctx_collega")}</Button>
        <Button variant="ghost" onClick={onDone}>{t("channels.annulla")}</Button>
      </div>
    </form>
  );
}

/**
 * «Conversazioni collegate»: the owner says which accounts are the same person, and their direct conversations go on
 * as one. Never by guessing, never for groups, never freer than the most restricted channel.
 */
export function ContextsPanel({ channels }: { channels: ChannelInfo[] }) {
  const contexts = useContexts();
  const [adding, setAdding] = useState(false);
  const linkable = linkableChannels(channels);
  const list = contexts.data ?? [];
  const titleId = useId();
  const heading = useRef<HTMLHeadingElement>(null);
  const opener = `${titleId}-link`;
  return (
    <section className="section" aria-labelledby={titleId}>
      <div className="section-head">
        <h2 id={titleId} ref={heading} tabIndex={-1}>{t("channels.ctx_titolo")}</h2>
        {linkable.length >= 2 && <div className="section-actions"><Button id={opener} icon="plus" aria-expanded={adding} onClick={() => setAdding(true)}>{t("channels.ctx_nuovo")}</Button></div>}
      </div>
      <p className="muted text-sm">{t("channels.ctx_intro")}</p>
      <p className="faint text-sm">{t("channels.ctx_regole")}</p>
      {linkable.length < 2 && list.length === 0 && <p className="faint text-sm">{t("channels.ctx_servono_canali")}</p>}
      {adding && <LinkForm channels={linkable} taken={takenSenders(list)} onDone={() => { setAdding(false); setTimeout(() => document.getElementById(opener)?.focus(), 0); }} />}
      {list.length === 0 && !adding && linkable.length >= 2 && <p className="faint text-sm">{t("channels.ctx_nessuno")}</p>}
      <div className="stack stack-lg">{list.map((c) => <ContextCard key={c.id} ctx={c} />)}</div>
    </section>
  );
}

import { useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { toast } from "../../../shared/lib/toast";
import type { AgentProfile, ChannelInfo, ChannelMode, ChannelPairing } from "../../../shared/api/types";
import { Button, Field, Icon, Input, Led, Pill, Segmented, Select, TagInput, Toggle } from "../../../shared/ui";
import { useDeleteChannel, usePairChannel, useTestChannel, useUpdateChannel } from "../api";
import { alwaysOnNote, downgradeNote, MODE_LABEL, MODE_NOTE, STATE_LABEL, diffPatch, enableProblem, ignoredNote, kindInfo, minutesLeft, stateTone } from "../model/channels";

import { t, tNode } from "../../../shared/i18n";
/** One channel: status, switch, set-up steps, who may talk, mode, briefing, test and (Telegram) pairing. */
export function ChannelCard({ channel, agents }: { channel: ChannelInfo; agents: AgentProfile[] }) {
  const info = kindInfo(channel.kind);
  const update = useUpdateChannel();
  const remove = useDeleteChannel();
  const test = useTestChannel();
  const pair = usePairChannel();
  const [name, setName] = useState(channel.name);
  const [mode, setMode] = useState<ChannelMode>(channel.mode);
  const [allowed, setAllowed] = useState(channel.allowed);
  const [settings, setSettings] = useState<Record<string, string>>(() =>
    Object.fromEntries(info.settings.map((f) => [f.key, typeof channel.settings[f.key] === "string" ? (channel.settings[f.key] as string) : ""])),
  );
  const [typed, setTyped] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{ ok: boolean; detail: string } | null>(null);
  const [code, setCode] = useState<ChannelPairing | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const patch = diffPatch(channel, { name, mode, allowed, settings, typedSecrets: typed });
  const dirty = Object.keys(patch).length > 0;
  const blocker = enableProblem(channel);
  const state = channel.status.state;
  const busy = update.isPending || remove.isPending;

  const save = () => {
    setError(null);
    update.mutate({ id: channel.id, patch }, {
      onSuccess: () => {
        setTyped({});
        toast(t("channels.canale_salvato"));
      },
      onError: (e) => setError(errorMessage(e)),
    });
  };
  const setEnabled = (on: boolean) => {
    setError(null);
    update.mutate({ id: channel.id, patch: { ...patch, enabled: on } }, {
      onSuccess: () => setTyped({}),
      onError: (e) => setError(errorMessage(e)),
    });
  };

  return (
    <article className="panel chn" aria-label={t("channels.canale", { name: channel.name })}>
      <header className="chn-head">
        <Led tone={stateTone(state)} />
        <div className="chn-title">
          <h2>{channel.name}</h2>
          <span className="muted text-sm">{info.label} · {STATE_LABEL[state]}</span>
        </div>
        <Toggle checked={channel.enabled} label={t("channels.canale_acceso", { name: channel.name })} disabled={busy || (!channel.enabled && blocker !== null)} onChange={setEnabled} />
      </header>
      {channel.status.detail && <p className={state === "error" ? "chn-error" : "muted text-sm"}>{channel.status.detail}</p>}
      {!channel.enabled && blocker && <p className="faint text-sm">{t("channels.per_accenderlo", { blocker })}</p>}
      <p className="faint text-xs">{t("channels.messaggi_trattati_conversazioni", { p: ignoredNote(channel.status.ignored), handled: channel.status.handled, conversations: channel.status.conversations })}</p>

      <details className="chn-steps">
        <summary>{t("channels.come_si_collega")}</summary>
        <ol>{info.steps.map((s) => <li key={s}>{s}</li>)}</ol>
      </details>

      <div className="chn-form">
        <Field label={t("channels.nome")} htmlFor={`chn-name-${channel.id}`}>
          <Input id={`chn-name-${channel.id}`} value={name} maxLength={60} onChange={(e) => setName(e.target.value)} />
        </Field>
        {info.secrets.map((f) => (
          <Field key={f.key} label={f.label} htmlFor={`chn-${channel.id}-${f.key}`} hint={channel.secrets[f.key] ? t("channels.gia_salvato_lascia_vuoto_per") : undefined}>
            <Input id={`chn-${channel.id}-${f.key}`} type="password" autoComplete="off" mono placeholder={channel.secrets[f.key] ? "••••••" : f.placeholder} value={typed[f.key] ?? ""} onChange={(e) => setTyped({ ...typed, [f.key]: e.target.value })} />
          </Field>
        ))}
        {info.settings.map((f) => (
          <Field key={f.key} label={f.label} htmlFor={`chn-${channel.id}-${f.key}`} hint={f.hint}>
            <Input id={`chn-${channel.id}-${f.key}`} mono placeholder={f.placeholder} value={settings[f.key] ?? ""} onChange={(e) => setSettings({ ...settings, [f.key]: e.target.value })} />
          </Field>
        ))}
        <Field label={info.allowedLabel} hint={info.allowedHint}>
          <TagInput label={info.allowedLabel} value={allowed} onChange={setAllowed} unique placeholder={info.allowedPlaceholder} />
        </Field>
        <Field label={t("channels.assistente_che_risponde")} htmlFor={`chn-agent-${channel.id}`}>
          <Select id={`chn-agent-${channel.id}`} value={channel.agent_id} disabled={busy} onChange={(e) => update.mutate({ id: channel.id, patch: { agent_id: e.target.value } }, { onError: (err) => setError(errorMessage(err)) })}>
            {agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </Select>
        </Field>
        <div className="field">
          <span className="chn-label">{t("channels.che_cosa_puo_fare_da")}</span>
          <Segmented<ChannelMode> label={t("channels.modalita_del_canale")} value={mode} onChange={setMode} options={(["read_only", "ask"] as const).map((m) => ({ id: m, label: MODE_LABEL[m] }))} />
          <span className="hint">{MODE_NOTE[mode]}</span>
          {channel.downgraded && <span className="chn-warn"><Icon name="alert" size={14} /> {downgradeNote()}</span>}
        </div>
        <label className="chn-switch">
          <Toggle checked={channel.briefing} label={t("channels.manda_qui_anche_il_riepilogo")} disabled={busy} onChange={(on) => update.mutate({ id: channel.id, patch: { briefing: on } }, { onError: (err) => setError(errorMessage(err)) })} />
          <span>{t("channels.il_riepilogo_del_mattino_va")}</span>
        </label>
      </div>

      {error && <p className="chn-error" role="alert">{error}</p>}
      {result && <p className={result.ok ? "chn-ok" : "chn-error"} role="status">{result.detail}</p>}
      {code && (
        <div className="chn-code" role="status">
          <span>{t("channels.scrivi_al_tuo_bot_questo", { p: Math.max(1, minutesLeft(code.expires_at)) })}</span>
          <code className="mono">{code.command}</code>
        </div>
      )}

      <div className="row">
        <Button variant="primary" disabled={!dirty || busy} onClick={save}>{t("channels.salva")}</Button>
        <Button icon="check" disabled={test.isPending} onClick={() => test.mutate(channel.id, { onSuccess: setResult, onError: (e) => setResult({ ok: false, detail: errorMessage(e) }) })}>{t("channels.prova")}</Button>
        {channel.kind === "telegram" && (
          <Button icon="key" disabled={pair.isPending || !channel.secrets.bot_token} onClick={() => pair.mutate(channel.id, { onSuccess: setCode, onError: (e) => setError(errorMessage(e)) })}>{t("channels.genera_il_codice")}</Button>
        )}
        <span className="grow" />
        {confirmDelete ? (
          <>
            <Button variant="danger" disabled={busy} onClick={() => remove.mutate(channel.id)}>{t("channels.si_elimina")}</Button>
            <Button variant="ghost" onClick={() => setConfirmDelete(false)}>{t("channels.annulla")}</Button>
          </>
        ) : (
          <Button variant="ghost" icon="trash" onClick={() => setConfirmDelete(true)}>{t("channels.elimina")}</Button>
        )}
      </div>
      <p className="faint text-xs">{alwaysOnNote()}</p>
      {channel.kind === "email" && <p className="faint text-xs">{tNode("channels.la_posta_da_altri_indirizzi", { pill: <Pill tone="neutral">{t("channels.dkim_spf")}</Pill> })}</p>}
    </article>
  );
}

import { useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { toast } from "../../../shared/lib/toast";
import type { AgentProfile, ChannelInfo, ChannelMode, ChannelPairing } from "../../../shared/api/types";
import { Button, Field, Icon, Input, Led, Pill, Segmented, Select, TagInput, Toggle } from "../../../shared/ui";
import { useDeleteChannel, usePairChannel, useTestChannel, useUpdateChannel } from "../api";
import { ALWAYS_ON_NOTE, DOWNGRADE_NOTE, MODE_LABEL, MODE_NOTE, STATE_LABEL, diffPatch, enableProblem, ignoredNote, kindInfo, minutesLeft, stateTone } from "../model/channels";

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
        toast("Canale salvato");
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
    <article className="panel chn" aria-label={`Canale ${channel.name}`}>
      <header className="chn-head">
        <Led tone={stateTone(state)} />
        <div className="chn-title">
          <h2>{channel.name}</h2>
          <span className="muted text-sm">{info.label} · {STATE_LABEL[state]}</span>
        </div>
        <Toggle checked={channel.enabled} label={`Canale ${channel.name} acceso`} disabled={busy || (!channel.enabled && blocker !== null)} onChange={setEnabled} />
      </header>
      {channel.status.detail && <p className={state === "error" ? "chn-error" : "muted text-sm"}>{channel.status.detail}</p>}
      {!channel.enabled && blocker && <p className="faint text-sm">Per accenderlo: {blocker}</p>}
      <p className="faint text-xs">{ignoredNote(channel.status.ignored)} · {channel.status.handled} messaggi trattati · {channel.status.conversations} conversazioni</p>

      <details className="chn-steps">
        <summary>Come si collega</summary>
        <ol>{info.steps.map((s) => <li key={s}>{s}</li>)}</ol>
      </details>

      <div className="chn-form">
        <Field label="Nome" htmlFor={`chn-name-${channel.id}`}>
          <Input id={`chn-name-${channel.id}`} value={name} maxLength={60} onChange={(e) => setName(e.target.value)} />
        </Field>
        {info.secrets.map((f) => (
          <Field key={f.key} label={f.label} htmlFor={`chn-${channel.id}-${f.key}`} hint={channel.secrets[f.key] ? "Già salvato: lascia vuoto per tenerlo. Non viene mai mostrato." : undefined}>
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
        <Field label="Assistente che risponde" htmlFor={`chn-agent-${channel.id}`}>
          <Select id={`chn-agent-${channel.id}`} value={channel.agent_id} disabled={busy} onChange={(e) => update.mutate({ id: channel.id, patch: { agent_id: e.target.value } }, { onError: (err) => setError(errorMessage(err)) })}>
            {agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </Select>
        </Field>
        <div className="field">
          <span className="chn-label">Che cosa può fare da qui</span>
          <Segmented<ChannelMode> label="Modalità del canale" value={mode} onChange={setMode} options={(["read_only", "ask"] as const).map((m) => ({ id: m, label: MODE_LABEL[m] }))} />
          <span className="hint">{MODE_NOTE[mode]}</span>
          {channel.downgraded && <span className="chn-warn"><Icon name="alert" size={14} /> {DOWNGRADE_NOTE}</span>}
        </div>
        <label className="chn-switch">
          <Toggle checked={channel.briefing} label="Manda qui anche il riepilogo del mattino" disabled={busy} onChange={(on) => update.mutate({ id: channel.id, patch: { briefing: on } }, { onError: (err) => setError(errorMessage(err)) })} />
          <span>Il riepilogo del mattino va anche su questo canale</span>
        </label>
      </div>

      {error && <p className="chn-error" role="alert">{error}</p>}
      {result && <p className={result.ok ? "chn-ok" : "chn-error"} role="status">{result.detail}</p>}
      {code && (
        <div className="chn-code" role="status">
          <span>Scrivi al tuo bot questo messaggio entro {Math.max(1, minutesLeft(code.expires_at))} minuti (vale una volta sola):</span>
          <code className="mono">{code.command}</code>
        </div>
      )}

      <div className="row">
        <Button variant="primary" disabled={!dirty || busy} onClick={save}>Salva</Button>
        <Button icon="check" disabled={test.isPending} onClick={() => test.mutate(channel.id, { onSuccess: setResult, onError: (e) => setResult({ ok: false, detail: errorMessage(e) }) })}>Prova</Button>
        {channel.kind === "telegram" && (
          <Button icon="key" disabled={pair.isPending || !channel.secrets.bot_token} onClick={() => pair.mutate(channel.id, { onSuccess: setCode, onError: (e) => setError(errorMessage(e)) })}>Genera il codice</Button>
        )}
        <span className="grow" />
        {confirmDelete ? (
          <>
            <Button variant="danger" disabled={busy} onClick={() => remove.mutate(channel.id)}>Sì, elimina</Button>
            <Button variant="ghost" onClick={() => setConfirmDelete(false)}>Annulla</Button>
          </>
        ) : (
          <Button variant="ghost" icon="trash" onClick={() => setConfirmDelete(true)}>Elimina</Button>
        )}
      </div>
      <p className="faint text-xs">{ALWAYS_ON_NOTE}</p>
      {channel.kind === "email" && <p className="faint text-xs">La posta da altri indirizzi, quella automatica e quella senza una firma verificata dal server viene ignorata. <Pill tone="neutral">DKIM / SPF</Pill></p>}
    </article>
  );
}

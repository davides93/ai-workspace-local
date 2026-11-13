import "../channels.css";
import { useEffect, useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { toast } from "../../../shared/lib/toast";
import type { MailAccount, MailAccountTest } from "../../../shared/api/types";
import { Button, Field, Input, QueryState } from "../../../shared/ui";
import { useMailAccounts, useSetMailAccounts, useTestMailAccount } from "../api";
import { MAIL_ACCOUNT_LIMIT, accountProblem, emptyAccount, hasStoredPassword, parseFolders } from "../model/channels";

import { t, t as tr } from "../../../shared/i18n";
function AccountForm({ account, onChange, onRemove }: { account: MailAccount; onChange: (a: MailAccount) => void; onRemove: () => void }) {
  const test = useTestMailAccount();
  const [result, setResult] = useState<MailAccountTest | null>(null);
  const [folders, setFolders] = useState(account.folders.join(", "));
  const uid = account.id || "nuovo";
  const problem = accountProblem(account);
  return (
    <div className="chn-account">
      <div className="chn-form">
        <Field label={t("channels.server_imap")} htmlFor={`imap-host-${uid}`}>
          <Input id={`imap-host-${uid}`} mono placeholder="imap.example.com" value={account.host} onChange={(e) => onChange({ ...account, host: e.target.value })} />
        </Field>
        <Field label={t("channels.porta")} htmlFor={`imap-port-${uid}`} hint={t("channels.993_e_la_porta_con")}>
          <Input id={`imap-port-${uid}`} type="number" min={1} max={65535} value={account.port} onChange={(e) => onChange({ ...account, port: Number(e.target.value) })} />
        </Field>
        <Field label={t("channels.utente")} htmlFor={`imap-user-${uid}`}>
          <Input id={`imap-user-${uid}`} mono autoComplete="off" value={account.username} onChange={(e) => onChange({ ...account, username: e.target.value })} />
        </Field>
        <Field label={t("channels.password")} htmlFor={`imap-pw-${uid}`} hint={hasStoredPassword(account) ? t("channels.gia_salvata_lascia_cosi_per") : t("channels.per_gmail_e_microsoft_serve")}>
          <Input id={`imap-pw-${uid}`} type="password" autoComplete="off" mono value={account.password} onChange={(e) => onChange({ ...account, password: e.target.value })} />
        </Field>
        <Field label={t("channels.cartelle_da_leggere")} htmlFor={`imap-folders-${uid}`} hint={t("channels.una_o_piu_separate_da")}>
          <Input id={`imap-folders-${uid}`} mono value={folders} onChange={(e) => { setFolders(e.target.value); onChange({ ...account, folders: parseFolders(e.target.value) }); }} />
        </Field>
      </div>
      {result && <p className={result.ok ? "chn-ok" : "chn-error"} role="status">{result.detail}{result.ok && result.folders.length > 0 && ` (${result.folders.map((f) => `${f.name}: ${f.recent}`).join(", ")})`}</p>}
      <div className="row">
        <Button icon="check" disabled={test.isPending || problem !== null} title={problem ?? undefined} onClick={() => test.mutate(account, { onSuccess: setResult, onError: (e) => setResult({ ok: false, detail: errorMessage(e), folders: [] }) })}>{t("channels.prova")}</Button>
        <Button variant="ghost" icon="trash" onClick={onRemove}>{t("channels.togli_l_account")}</Button>
      </div>
    </div>
  );
}

/**
 * «Account di posta (IMAP)» in Settings → «Calendario e posta»: read-only access to a mailbox on a server.
 * The password is kept by the app and never shown again; the assistant reads, it never changes or sends.
 */
export function MailAccountsEditor() {
  const saved = useMailAccounts();
  const save = useSetMailAccounts();
  const [draft, setDraft] = useState<MailAccount[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (saved.data && draft === null) setDraft(saved.data);
  }, [saved.data, draft]);
  const list = draft ?? saved.data ?? [];
  const dirty = draft !== null && JSON.stringify(draft) !== JSON.stringify(saved.data);
  const problem = list.map(accountProblem).find((p) => p !== null) ?? null;
  return (
    <section className="section" aria-label={t("channels.account_di_posta_imap")}>
      <div className="section-head">
        <h2>{t("channels.account_di_posta_imap")}</h2>
        <span className="muted">{t("channels.l_assistente_legge_la_posta")}</span>
      </div>
      <p className="faint text-sm">{t("channels.gmail_e_microsoft_l_accesso")}</p>
      <QueryState query={saved}>
        {() => (
          <div className="chn-accounts">
            {list.length === 0 && <p className="muted">{t("channels.nessun_account_aggiungine_uno_per")}</p>}
            {list.map((a, i) => (
              <AccountForm key={a.id || `n${i}`} account={a} onChange={(next) => setDraft(list.map((x, j) => (j === i ? next : x)))} onRemove={() => setDraft(list.filter((_, j) => j !== i))} />
            ))}
            {error && <p className="chn-error" role="alert">{error}</p>}
            <div className="row">
              <Button icon="plus" disabled={list.length >= MAIL_ACCOUNT_LIMIT} onClick={() => setDraft([...list, emptyAccount()])}>{t("channels.aggiungi_un_account")}</Button>
              <Button variant="primary" disabled={!dirty || save.isPending || problem !== null} title={problem ?? undefined}
                onClick={() => { setError(null); save.mutate(list, { onSuccess: (next) => { setDraft(next); toast(tr("channels.account_di_posta_salvati")); }, onError: (e) => setError(errorMessage(e)) }); }}>
                {t("channels.salva_gli_account")}</Button>
            </div>
          </div>
        )}
      </QueryState>
    </section>
  );
}

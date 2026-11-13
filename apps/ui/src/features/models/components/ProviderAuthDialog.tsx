import { useState, type FormEvent, type ReactNode } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { toast } from "../../../shared/lib/toast";
import { Button, Dialog, ErrorState, Field, Icon, Input, Pill, SkeletonLines } from "../../../shared/ui";
import type { ModelProviderInfo } from "../../../shared/api/types";
import { useModelProviders, useSetProviderAuth } from "../api";
import { authPayload, hasAuthChanges, hostOf, validateEndpoint } from "../model/forms";
import { authBadge } from "../model/providers";

import { t } from "../../../shared/i18n";
const FORM_ID = "provider-auth-form";

/**
 * Settings of one provider: access token (write-only, never shown back) and, where the provider has
 * one, the endpoint or mirror. Saving sends only what changed, as the backend expects.
 */
export function ProviderAuthDialog({ providerId, onClose }: { providerId: string; onClose: () => void }) {
  const providers = useModelProviders();
  const info = providers.data?.find((p) => p.id === providerId);
  if (!info) {
    return (
      <Dialog title={t("models.accesso_al_provider")} size="md" onClose={onClose} footer={<Button onClick={onClose}>{t("models.chiudi")}</Button>}>
        {providers.isError ? <ErrorState error={providers.error} onRetry={() => void providers.refetch()} /> : <SkeletonLines lines={3} />}
      </Dialog>
    );
  }
  return <AuthDialog info={info} onClose={onClose} />;
}

function AuthDialog({ info, onClose }: { info: ModelProviderInfo; onClose: () => void }) {
  const set = useSetProviderAuth();
  const [token, setToken] = useState("");
  const [endpoint, setEndpoint] = useState(info.endpoint);
  const [reset, setReset] = useState(false);

  const badge = authBadge(info);
  const usesToken = info.auth !== "none";
  const usesEndpoint = info.endpoint !== "";
  const endpointError = reset ? null : validateEndpoint(endpoint);
  const payload = authPayload({ token, endpoint, resetEndpoint: reset }, info);
  const dirty = hasAuthChanges(payload);
  const host = hostOf(reset || !endpoint.trim() ? info.endpoint : endpoint);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!dirty || endpointError) return;
    set.mutate(
      { id: info.id, token: payload.token, endpoint: payload.endpoint },
      {
        onSuccess: () => {
          toast(payload.token ? t("models.token_di_salvato", { name: info.name }) : t("models.impostazioni_del_provider_aggiornate"));
          onClose();
        },
      },
    );
  };

  const removeToken = () =>
    set.mutate({ id: info.id, token: null }, { onSuccess: () => { toast(t("models.token_di_rimosso", { name: info.name })); onClose(); } });

  return (
    <Dialog
      title={t("models.accesso_a", { name: info.name })}
      size="md"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t("models.annulla")}</Button>
          <Button type="submit" form={FORM_ID} variant="primary" icon="check" disabled={!dirty || !!endpointError || set.isPending}>
            {set.isPending ? t("models.salvo") : t("models.salva")}
          </Button>
        </>
      }
    >
      <form id={FORM_ID} className="form-grid" onSubmit={submit} noValidate>
        <p className="muted text-sm">{info.description}</p>

        {usesToken && (
          <div className="stack">
            <div className="row row-between">
              <label className="field-label" htmlFor="provider-token">{t("models.token_di_accesso")}</label>
              <Pill tone={badge.tone} title={badge.detail}>
                {info.auth_configured && <Icon name="key" size={12} />}
                {badge.label}
              </Pill>
            </div>
            <Input
              id="provider-token" type="password" mono autoComplete="off" spellCheck={false} data-autofocus
              value={token} onChange={(e) => setToken(e.target.value)}
              placeholder={info.auth_configured ? t("models.lascia_vuoto_per_tenere_quello") : t("models.incolla_il_token")}
              aria-describedby="provider-token-hint"
            />
            <span id="provider-token-hint" className="faint text-xs">
              {host ? <>Salvato solo su questo device e inviato soltanto a <b>{host}</b>. </> : t("models.salvato_solo_su_questo_device")}
              Una volta salvato non viene più mostrato.
              {info.auth === "optional" && t("models.serve_per_i_repository_gated")}
            </span>
            {info.auth_configured && (
              <div className="row">
                <Button variant="danger" size="sm" icon="trash" onClick={removeToken} disabled={set.isPending}>{t("models.rimuovi_il_token")}</Button>
              </div>
            )}
          </div>
        )}

        {usesEndpoint && (
          <Field
            label={t("models.endpoint_o_mirror")} htmlFor="provider-endpoint" error={endpointError ?? undefined}
            hint={t("models.indirizzo_base_del_servizio_cambialo")}
          >
            <Input
              id="provider-endpoint" type="url" mono inputMode="url" autoComplete="off" spellCheck={false}
              value={reset ? "" : endpoint} placeholder={reset ? t("models.verra_usato_l_indirizzo_predefinito") : "https://…"}
              disabled={reset} onChange={(e) => setEndpoint(e.target.value)} aria-invalid={endpointError ? true : undefined}
            />
            <label className="mh-reset">
              <input type="checkbox" checked={reset} onChange={(e) => setReset(e.target.checked)} />
              {t("models.ripristina_l_indirizzo_predefinito")}</label>
          </Field>
        )}

        {!usesToken && !usesEndpoint && <p className="muted">{t("models.questo_provider_non_ha_impostazioni")}</p>}
        {set.isError && <p className="field-error" role="alert">{errorMessage(set.error)}</p>}
        {set.isPending && payload.token && <p className="faint text-sm" role="status">{t("models.verifico_il_token_con_il")}</p>}
      </form>
    </Dialog>
  );
}

/** Opens the settings dialog of a provider from anywhere: call `open(id)` and render `dialog`. */
export function useProviderAuthDialog(): { open: (providerId: string) => void; dialog: ReactNode } {
  const [id, setId] = useState<string | null>(null);
  return { open: setId, dialog: id ? <ProviderAuthDialog providerId={id} onClose={() => setId(null)} /> : null };
}

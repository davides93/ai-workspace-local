import { setQuery, useQueryParams } from "../../../app/router";
import { Button, Icon, QueryState } from "../../../shared/ui";
import { useModelProviders } from "../api";
import { discoverProviders, hasSettings, isUrlProvider, pickProvider, providerNote } from "../model/providers";
import { ModelSearch } from "./ModelSearch";
import { ProviderPicker } from "./ProviderPicker";
import { useProviderAuthDialog } from "./ProviderAuthDialog";
import { UrlPullForm } from "./UrlPullForm";

/** Scopri: pick a provider, then search it live (or paste an address for the direct URL provider). */
export function DiscoverTab() {
  const providers = useModelProviders();
  const requested = useQueryParams().provider;
  const auth = useProviderAuthDialog();
  return (
    <QueryState query={providers}>
      {(all) => {
        const offered = discoverProviders(all);
        const provider = pickProvider(offered, requested);
        const info = offered.find((p) => p.id === provider);
        const note = providerNote(provider);
        const thin = !!info && !note && !info.homepage && !hasSettings(info);
        return (
          <div className="stack stack-lg">
            <h2 className="sr-only">Scopri nuovi modelli</h2>
            <ProviderPicker
              providers={offered} value={provider}
              onChange={(id) => setQuery({ provider: id, q: "", format: "", sort: "", url: "" })}
            />
            {info && (
              <section className={thin ? "mh-provider-bar thin" : "mh-provider-bar"} aria-label={`Informazioni su ${info.name}`}>
                <div className="grow stack stack-sm">
                  {/* The cards carry the description on wide screens; on phones they are too small for it. */}
                  <p className="text-sm mh-bar-desc">{info.description}</p>
                  {note && <p className="faint text-sm">{note}</p>}
                  {info.homepage && (
                    <a className="link-more text-sm" href={info.homepage} target="_blank" rel="noreferrer noopener">
                      Sito di {info.name} <Icon name="external" size={13} />
                      <span className="sr-only"> (si apre in una nuova scheda)</span>
                    </a>
                  )}
                </div>
                {hasSettings(info) && (
                  <Button size="sm" icon="key" onClick={() => auth.open(info.id)}>
                    {info.auth !== "none" ? "Token e impostazioni" : "Impostazioni"}
                  </Button>
                )}
              </section>
            )}
            {isUrlProvider(provider) ? <UrlPullForm /> : <ModelSearch key={provider} provider={provider} info={info} onSetToken={() => auth.open(provider)} />}
            {auth.dialog}
          </div>
        );
      }}
    </QueryState>
  );
}

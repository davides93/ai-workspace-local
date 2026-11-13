import { Component, type ErrorInfo, type ReactNode } from "react";
import { ErrorState } from "../../shared/ui";

import { t } from "../../shared/i18n";
/** Catches render errors (and failed lazy chunks) of one view so the shell stays usable. */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: unknown }> {
  state = { error: null as unknown };

  static getDerivedStateFromError(error: unknown) {
    return { error };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("Errore nella vista", error, info.componentStack);
  }

  render() {
    if (this.state.error === null) return this.props.children;
    return (
      <div className="page">
        <ErrorState title={t("shell.questa_pagina_non_si_e")} error={this.state.error} onRetry={() => this.setState({ error: null })} />
      </div>
    );
  }
}

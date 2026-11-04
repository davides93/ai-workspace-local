import { Component, type ErrorInfo, type ReactNode } from "react";
import { ErrorState } from "../../shared/ui";

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
        <ErrorState title="Questa pagina non si è caricata" error={this.state.error} onRetry={() => this.setState({ error: null })} />
      </div>
    );
  }
}

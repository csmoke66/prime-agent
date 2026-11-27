import { Component, type ReactNode } from "react";

/** Contains a render failure to its own area: the rest of the app keeps working and the error is shown in place. */
export class ErrorBoundary extends Component<{ children: ReactNode; resetKey?: string }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidUpdate(previous: { resetKey?: string }) {
    if (this.state.error && previous.resetKey !== this.props.resetKey) this.setState({ error: null });
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="grid h-full place-items-center p-6">
        <div className="max-w-lg rounded-md border border-error/40 bg-surface p-4 text-[13px]">
          <div className="font-semibold text-error">This view failed to render</div>
          <pre className="mt-2 whitespace-pre-wrap break-words font-mono text-[11.5px] text-muted">{this.state.error.message}</pre>
          <button className="bh-btn mt-3" onClick={() => this.setState({ error: null })}>Try again</button>
        </div>
      </div>
    );
  }
}

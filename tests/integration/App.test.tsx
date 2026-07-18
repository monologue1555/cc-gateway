import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/App";
import { resetProviderState } from "../msw/state";
import { emitTauriEvent } from "../msw/tauriMocks";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/components/gateway/ClaudeGatewayDashboard", () => ({
  ClaudeGatewayDashboard: () => (
    <div data-testid="claude-gateway-dashboard">
      canonical AnyRouter dashboard
    </div>
  ),
}));

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: { onClick: () => void }) => (
    <button onClick={onClick}>update-badge</button>
  ),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: vi.fn().mockResolvedValue(false),
    onResized: vi.fn().mockResolvedValue(() => {}),
    setDecorations: vi.fn().mockResolvedValue(undefined),
  }),
}));

const renderApp = (AppComponent: ComponentType) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <AppComponent />
      </Suspense>
    </QueryClientProvider>,
  );
};

describe("CC Gateway App integration", () => {
  beforeEach(() => {
    resetProviderState();
    localStorage.clear();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
  });

  it("uses the canonical Claude Gateway dashboard as the only home surface", async () => {
    renderApp(App);

    await screen.findByTestId("claude-gateway-dashboard");
    expect(
      screen.getByText("canonical AnyRouter dashboard"),
    ).toBeInTheDocument();

    for (const removedSurface of [
      "switch-codex",
      "switch-openclaw",
      "provider-list",
      "add-provider-dialog",
    ]) {
      expect(screen.queryByTestId(removedSurface)).not.toBeInTheDocument();
      expect(screen.queryByText(removedSurface)).not.toBeInTheDocument();
    }
  });

  it("ignores legacy background sync events without changing the Claude surface", async () => {
    renderApp(App);
    await screen.findByTestId("claude-gateway-dashboard");

    expect(() => {
      emitTauriEvent("webdav-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("webdav-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "network timeout",
    });
    emitTauriEvent("s3-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "s3 timeout",
    });

    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(screen.getByTestId("claude-gateway-dashboard")).toBeInTheDocument();
  });
});

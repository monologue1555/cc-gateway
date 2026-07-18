import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeGatewayDashboard } from "@/components/gateway/ClaudeGatewayDashboard";
import { server } from "../msw/server";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

function runtimeState(claudeCodeEnabled: boolean) {
  return {
    claudeCodeEnabled,
    claudeDesktopEnabled: false,
    backendEnabled: false,
    runtime: {
      phase: claudeCodeEnabled ? "ready" : "disabled",
      consumers: claudeCodeEnabled ? ["claude-code"] : [],
      lastError: null,
    },
  };
}

function renderDashboard() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ClaudeGatewayDashboard />
    </QueryClientProvider>,
  );
}

describe("ClaudeGatewayDashboard", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
  });

  it("shares the canonical profile across three consumers and shows diagnostics", async () => {
    let codeEnabled = false;
    server.use(
      http.post("http://tauri.local/get_claude_adapter_state", () =>
        HttpResponse.json(runtimeState(codeEnabled)),
      ),
      http.post(
        "http://tauri.local/set_claude_code_adapter_enabled",
        async ({ request }) => {
          const body = (await request.json()) as { enabled: boolean };
          codeEnabled = body.enabled;
          return HttpResponse.json(runtimeState(codeEnabled));
        },
      ),
    );

    renderDashboard();

    expect(
      await screen.findByText("AnyRouter Claude 控制台"),
    ).toBeInTheDocument();
    expect(await screen.findByDisplayValue("https://anyrouter.top")).toBeInTheDocument();
    expect(screen.getByText("Claude Code")).toBeInTheDocument();
    expect(screen.getByText("Claude Desktop")).toBeInTheDocument();
    expect(screen.getByText("Backend Gateway")).toBeInTheDocument();

    const codeSwitch = await screen.findByRole("switch", {
      name: "Claude Code启用",
    });
    fireEvent.click(codeSwitch);

    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "Claude Code停用" }),
      ).toBeChecked(),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Claude Code 状态已更新");

    expect(await screen.findByText("req-mock-1")).toBeInTheDocument();
    expect(screen.getByText("claude-opus-4-8[1M]")).toBeInTheDocument();
    expect(screen.getByText("→ claude-opus-4-8")).toBeInTheDocument();
    expect(screen.queryByText("Codex")).not.toBeInTheDocument();
    expect(screen.queryByText("Gemini")).not.toBeInTheDocument();
    expect(toastErrorMock).not.toHaveBeenCalled();
  });
});

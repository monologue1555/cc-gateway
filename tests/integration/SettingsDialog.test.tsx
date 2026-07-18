import React, { Suspense } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { SettingsPage } from "@/components/settings/SettingsPage";
import {
  resetProviderState,
  getSettings,
  getAppConfigDirOverride,
} from "../msw/state";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) =>
    open ? <div data-testid="dialog-root">{children}</div> : null,
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
  DialogDescription: ({ children }: any) => <div>{children}</div>,
}));

const TabsContext = React.createContext<{
  value: string;
  onValueChange?: (value: string) => void;
}>({
  value: "general",
});

vi.mock("@/components/ui/tabs", () => {
  return {
    Tabs: ({ value, onValueChange, children }: any) => (
      <TabsContext.Provider value={{ value, onValueChange }}>
        {children}
      </TabsContext.Provider>
    ),
    TabsList: ({ children }: any) => <div>{children}</div>,
    TabsTrigger: ({ value, children }: any) => {
      const ctx = React.useContext(TabsContext);
      return (
        <button type="button" onClick={() => ctx.onValueChange?.(value)}>
          {children}
        </button>
      );
    },
    TabsContent: ({ value, children }: any) => {
      const ctx = React.useContext(TabsContext);
      return ctx.value === value ? (
        <div data-testid={`tab-${value}`}>{children}</div>
      ) : null;
    },
  };
});

vi.mock("@/components/settings/LanguageSettings", () => ({
  LanguageSettings: ({ value, onChange }: any) => (
    <div>
      <span>language:{value}</span>
      <button onClick={() => onChange("en")}>change-language</button>
    </div>
  ),
}));

vi.mock("@/components/settings/ThemeSettings", () => ({
  ThemeSettings: () => <div data-testid="theme-settings">theme</div>,
}));

vi.mock("@/components/settings/WindowSettings", () => ({
  WindowSettings: ({ onChange }: any) => (
    <button onClick={() => onChange({ minimizeToTrayOnClose: false })}>
      window-settings
    </button>
  ),
}));

vi.mock("@/components/settings/DirectorySettings", async () => {
  const actual = await vi.importActual<
    typeof import("@/components/settings/DirectorySettings")
  >("@/components/settings/DirectorySettings");
  return actual;
});

vi.mock("@/components/settings/AboutSection", () => ({
  AboutSection: ({ isPortable }: any) => <div>about:{String(isPortable)}</div>,
}));

const renderDialog = (
  props?: Partial<React.ComponentProps<typeof SettingsPage>>,
) => {
  const client = new QueryClient();
  return render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <SettingsPage open onOpenChange={() => {}} {...props} />
      </Suspense>
    </QueryClientProvider>,
  );
};

beforeEach(() => {
  resetProviderState();
  toastSuccessMock.mockReset();
  toastErrorMock.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("SettingsPage integration", () => {
  it("loads default settings from MSW", async () => {
    renderDialog();

    await waitFor(() =>
      expect(screen.getByText("language:zh")).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByText("settings.tabAdvanced"));
    fireEvent.click(screen.getByText("settings.advanced.configDir.title"));
    const appInput = await screen.findByPlaceholderText(
      "settings.browsePlaceholderApp",
    );
    expect((appInput as HTMLInputElement).value).toBe("/home/mock/.cc-gateway");
  });

  it("previews and selectively imports only legacy Claude data", async () => {
    const onImportSuccess = vi.fn();
    renderDialog({ onImportSuccess });

    await waitFor(() =>
      expect(screen.getByText("language:zh")).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByText("settings.tabAdvanced"));
    fireEvent.click(screen.getByText("settings.advanced.data.title"));
    fireEvent.click(screen.getByText("选择旧数据库并预览"));
    await screen.findByText("cc-switch.db");
    await screen.findByText("仅导入预览中的 Claude 数据");

    fireEvent.click(screen.getByText("仅导入预览中的 Claude 数据"));
    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalled());
    await waitFor(() => expect(onImportSuccess).toHaveBeenCalled(), {
      timeout: 4000,
    });
    expect(getSettings().language).toBe("zh");
    expect(screen.getByText("选择性导入完成")).toBeInTheDocument();
    expect(screen.queryByText("settings.exportConfig")).not.toBeInTheDocument();
  });

  it("saves settings and handles restart prompt", async () => {
    renderDialog();

    await waitFor(() =>
      expect(screen.getByText("language:zh")).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByText("settings.tabAdvanced"));
    fireEvent.click(screen.getByText("settings.advanced.configDir.title"));
    const appInput = await screen.findByPlaceholderText(
      "settings.browsePlaceholderApp",
    );
    fireEvent.change(appInput, { target: { value: "/custom/app" } });
    fireEvent.click(screen.getByText("common.save"));

    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalled());
    await screen.findByText("settings.restartRequired");
    fireEvent.click(screen.getByText("settings.restartLater"));
    await waitFor(() =>
      expect(
        screen.queryByText("settings.restartRequired"),
      ).not.toBeInTheDocument(),
    );

    expect(getAppConfigDirOverride()).toBe("/custom/app");
  });

  it("allows browsing and resetting directories", async () => {
    renderDialog();

    await waitFor(() =>
      expect(screen.getByText("language:zh")).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByText("settings.tabAdvanced"));
    fireEvent.click(screen.getByText("settings.advanced.configDir.title"));

    const browseButtons = screen.getAllByTitle("settings.browseDirectory");
    const resetButtons = screen.getAllByTitle("settings.resetDefault");

    const appInput = (await screen.findByPlaceholderText(
      "settings.browsePlaceholderApp",
    )) as HTMLInputElement;
    expect(appInput.value).toBe("/home/mock/.cc-gateway");

    fireEvent.click(browseButtons[0]);
    await waitFor(() =>
      expect(appInput.value).toBe("/home/mock/.cc-gateway/picked"),
    );

    fireEvent.click(resetButtons[0]);
    await waitFor(() => expect(appInput.value).toBe("/home/mock/.cc-gateway"));

    const claudeInput = (await screen.findByPlaceholderText(
      "settings.browsePlaceholderClaude",
    )) as HTMLInputElement;
    fireEvent.change(claudeInput, { target: { value: "/custom/claude" } });
    await waitFor(() => expect(claudeInput.value).toBe("/custom/claude"));

    fireEvent.click(browseButtons[1]);
    await waitFor(() =>
      expect(claudeInput.value).toBe("/custom/claude/picked"),
    );

    fireEvent.click(resetButtons[1]);
    await waitFor(() => expect(claudeInput.value).toBe("/home/mock/.claude"));
  });

  it("does not expose legacy whole-database import or export actions", async () => {
    renderDialog();

    await waitFor(() =>
      expect(screen.getByText("language:zh")).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByText("settings.tabAdvanced"));
    fireEvent.click(screen.getByText("settings.advanced.data.title"));

    expect(screen.getByText("从旧 CC Switch 选择性导入")).toBeInTheDocument();
    expect(
      screen.getByText(/导入只更新 CC Gateway 自己的数据库/),
    ).toBeInTheDocument();
    expect(screen.queryByText("settings.import")).not.toBeInTheDocument();
    expect(screen.queryByText("settings.exportConfig")).not.toBeInTheDocument();
  });
});

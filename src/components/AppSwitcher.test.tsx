import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AppSwitcher } from "./AppSwitcher";

describe("CC Gateway application switcher", () => {
  it("exposes only Claude Code and Claude Desktop", () => {
    render(<AppSwitcher activeApp="claude" onSwitch={vi.fn()} />);

    expect(screen.getByText("Claude Code")).toBeInTheDocument();
    expect(screen.getByText("Claude Desktop")).toBeInTheDocument();
    for (const hiddenApp of [
      "Codex",
      "Gemini",
      "Grok Build",
      "OpenCode",
      "OpenClaw",
      "Hermes",
    ]) {
      expect(screen.queryByText(hiddenApp)).not.toBeInTheDocument();
    }
  });
});

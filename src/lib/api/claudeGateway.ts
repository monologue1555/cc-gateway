import { invoke } from "@tauri-apps/api/core";
import type {
  ClaudeAdapterState,
  GatewayDiagnosticEntry,
  LegacyImportOptions,
  LegacyImportPreview,
  LegacyImportResult,
} from "@/types/claudeGateway";

export const claudeGatewayApi = {
  getAdapterState(): Promise<ClaudeAdapterState> {
    return invoke("get_claude_adapter_state");
  },

  setClaudeCodeEnabled(enabled: boolean): Promise<ClaudeAdapterState> {
    return invoke("set_claude_code_adapter_enabled", { enabled });
  },

  setClaudeDesktopEnabled(enabled: boolean): Promise<ClaudeAdapterState> {
    return invoke("set_claude_desktop_adapter_enabled", { enabled });
  },

  getDiagnostics(): Promise<GatewayDiagnosticEntry[]> {
    return invoke("get_gateway_diagnostics");
  },

  previewLegacyImport(filePath: string): Promise<LegacyImportPreview> {
    return invoke("preview_legacy_cc_switch_import", { filePath });
  },

  importLegacyDatabase(
    filePath: string,
    options: LegacyImportOptions,
  ): Promise<LegacyImportResult> {
    return invoke("import_legacy_cc_switch_database", { filePath, options });
  },
};

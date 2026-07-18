import { invoke } from "@tauri-apps/api/core";
import type {
  ClaudeConnectionProfileInput,
  ClaudeConnectionProfileState,
  ClaudeConnectionTestInput,
  ClaudeConnectionTestSummary,
  ClaudeModelTestResult,
} from "@/types/claudeConnectionProfile";

export const claudeConnectionProfileApi = {
  get(): Promise<ClaudeConnectionProfileState> {
    return invoke("get_claude_connection_profile");
  },

  update(
    input: ClaudeConnectionProfileInput,
  ): Promise<ClaudeConnectionProfileState> {
    return invoke("update_claude_connection_profile", { input });
  },

  testOne(input: ClaudeConnectionTestInput): Promise<ClaudeModelTestResult> {
    return invoke("test_claude_connection_model", { input });
  },

  testAll(): Promise<ClaudeConnectionTestSummary> {
    return invoke("test_all_claude_connection_models");
  },
};

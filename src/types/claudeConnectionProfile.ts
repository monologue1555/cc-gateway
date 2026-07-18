export type ClaudeAuthMode = "bearer";
export type ClaudeApiFormat = "anthropic";
export type ClaudeModelRole =
  | "opus"
  | "fable"
  | "sonnet"
  | "haiku"
  | "subagent"
  | "fallback";

export interface ClaudeModelRoute {
  role: ClaudeModelRole;
  displayName: string;
  clientModelId: string;
  upstreamModelId: string;
  supports1m: boolean;
}

export type ClaudeConnectionTestCategory =
  | "success"
  | "configuration"
  | "authentication_failed"
  | "model_unavailable"
  | "protocol_error"
  | "proxy_interference"
  | "timeout"
  | "network_error"
  | "upstream_unavailable";

export interface ClaudeModelTestResult {
  success: boolean;
  role: ClaudeModelRole;
  requestedModel: string;
  upstreamModel: string;
  responseModel?: string;
  statusCode?: number;
  latencyMs: number;
  category: ClaudeConnectionTestCategory;
  message: string;
}

export interface ClaudeConnectionTestSummary {
  testedAtMs: number;
  success: boolean;
  results: ClaudeModelTestResult[];
}

export interface ClaudeConnectionProfileState {
  enabled: boolean;
  baseUrl: string;
  authMode: ClaudeAuthMode;
  apiFormat: ClaudeApiFormat;
  hasApiKey: boolean;
  maskedApiKey: string;
  models: ClaudeModelRoute[];
  lastTest?: ClaudeConnectionTestSummary;
}

export interface ClaudeConnectionProfileInput {
  enabled: boolean;
  baseUrl: string;
  /** Omit to preserve the stored key; send a string to replace it. */
  apiKey?: string;
  models: ClaudeModelRoute[];
}

export interface ClaudeConnectionTestInput {
  role: ClaudeModelRole;
}

import type { ClaudeModelRoute } from "@/types/claudeConnectionProfile";

export type ClaudeGatewayPhase =
  | "disabled"
  | "starting"
  | "ready"
  | "degraded"
  | "stopping";

export type ClaudeGatewayConsumer =
  | "claude-code"
  | "claude-desktop"
  | "backend";

export interface ClaudeRuntimeStatus {
  phase: ClaudeGatewayPhase;
  consumers: ClaudeGatewayConsumer[];
  lastError?: string | null;
}

export interface ClaudeAdapterState {
  claudeCodeEnabled: boolean;
  claudeDesktopEnabled: boolean;
  backendEnabled: boolean;
  runtime: ClaudeRuntimeStatus;
}

export interface GatewayDiagnosticEntry {
  requestId: string;
  protocol: string;
  requestedModel: string;
  upstreamModel: string;
  provider: string;
  status: string;
  firstByteMs?: number | null;
  endReason: string;
  timestamp: string;
}

export interface LegacyProviderCounts {
  claude: number;
  claudeDesktop: number;
}

export interface LegacyExcludedCounts {
  otherAgentProviders: number;
  localGatewayProviders: number;
  gatewaySettings: number;
  proxyOrTakeoverRows: number;
  syncSettings: number;
  historyOrUsageRows: number;
}

export interface LegacyCanonicalCandidatePreview {
  sourceAppType: string;
  sourceProviderId: string;
  sourceProviderName: string;
  baseUrl: string;
  hasApiKey: boolean;
  maskedApiKey: string;
  models: ClaudeModelRoute[];
}

export interface LegacyImportPreview {
  providers: LegacyProviderCounts;
  mcpServers: number;
  skills: number;
  prompts: number;
  profiles: number;
  excluded: LegacyExcludedCounts;
  canonicalCandidate?: LegacyCanonicalCandidatePreview | null;
  warnings: string[];
}

export interface LegacyImportOptions {
  replaceCanonicalProfile: boolean;
}

export interface LegacyImportCounts {
  providers: LegacyProviderCounts;
  mcpServers: number;
  skills: number;
  prompts: number;
  profiles: number;
}

export interface LegacyCanonicalImportResult {
  imported: boolean;
  replacedExisting: boolean;
  reason: string;
  candidate?: LegacyCanonicalCandidatePreview | null;
}

export interface LegacyImportResult {
  imported: LegacyImportCounts;
  excluded: LegacyExcludedCounts;
  canonicalProfile: LegacyCanonicalImportResult;
  liveConfigurationTouched: boolean;
  warnings: string[];
}

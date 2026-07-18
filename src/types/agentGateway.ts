export type AgentGatewayProtocol = "anthropic" | "responses" | "chat";

export interface AgentGatewayEndpoints {
  anthropic: string;
  responses: string;
  chat: string;
}

export interface AgentGatewayModel {
  id: string;
  label?: string;
  supports1m: boolean;
  upstreamModel?: string;
}

export interface AgentGatewayState {
  enabled: boolean;
  compatible: boolean;
  compatibilityMessage?: string | null;
  listenAddress: string;
  listenPort: number;
  endpoints: AgentGatewayEndpoints;
  maskedToken: string;
  currentProviderId: string | null;
  currentProviderName?: string | null;
  autoFailoverEnabled?: boolean;
  emulateClaudeCode: boolean;
  models: AgentGatewayModel[];
}

export interface AgentGatewayConfigInput {
  enabled: boolean;
  emulateClaudeCode: boolean;
}

export interface AgentGatewayTokenResult {
  token: string;
  maskedToken: string;
}

export interface AgentGatewayTestInput {
  protocol: AgentGatewayProtocol;
  model: string;
}

export interface AgentGatewayTestResult {
  success: boolean;
  protocol: AgentGatewayProtocol;
  latencyMs: number;
  model: string;
  message?: string;
}

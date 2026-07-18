import { invoke } from "@tauri-apps/api/core";
import type {
  AgentGatewayConfigInput,
  AgentGatewayState,
  AgentGatewayTestInput,
  AgentGatewayTestResult,
  AgentGatewayTokenResult,
} from "@/types/agentGateway";

export const agentGatewayApi = {
  async getState(): Promise<AgentGatewayState> {
    return invoke("get_agent_gateway_state");
  },

  async updateConfig(
    input: AgentGatewayConfigInput,
  ): Promise<AgentGatewayState> {
    return invoke("update_agent_gateway_config", { input });
  },

  async regenerateToken(): Promise<AgentGatewayTokenResult> {
    return invoke("regenerate_agent_gateway_token");
  },

  async testProtocol(
    input: AgentGatewayTestInput,
  ): Promise<AgentGatewayTestResult> {
    return invoke("test_agent_gateway_protocol", { input });
  },
};

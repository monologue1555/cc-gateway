import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { agentGatewayApi } from "@/lib/api/agentGateway";
import type {
  AgentGatewayConfigInput,
  AgentGatewayState,
  AgentGatewayTestInput,
} from "@/types/agentGateway";

export const agentGatewayKeys = {
  all: ["agentGateway"] as const,
  state: ["agentGateway", "state"] as const,
};

export function useAgentGatewayState() {
  return useQuery({
    queryKey: agentGatewayKeys.state,
    queryFn: () => agentGatewayApi.getState(),
    refetchInterval: 5000,
  });
}

export function useUpdateAgentGatewayConfig() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: AgentGatewayConfigInput) =>
      agentGatewayApi.updateConfig(input),
    onSuccess: (state) => {
      queryClient.setQueryData(agentGatewayKeys.state, state);
    },
  });
}

export function useRegenerateAgentGatewayToken() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => agentGatewayApi.regenerateToken(),
    onSuccess: (result) => {
      queryClient.setQueryData<AgentGatewayState>(
        agentGatewayKeys.state,
        (previous) =>
          previous
            ? { ...previous, maskedToken: result.maskedToken }
            : previous,
      );
    },
  });
}

export function useTestAgentGatewayProtocol() {
  return useMutation({
    mutationFn: (input: AgentGatewayTestInput) =>
      agentGatewayApi.testProtocol(input),
  });
}

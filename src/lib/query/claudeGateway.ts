import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { claudeGatewayApi } from "@/lib/api/claudeGateway";
import { agentGatewayKeys } from "@/lib/query/agentGateway";
import { claudeConnectionProfileKeys } from "@/lib/query/claudeConnectionProfile";

export const claudeGatewayKeys = {
  all: ["claudeGateway"] as const,
  adapters: ["claudeGateway", "adapters"] as const,
  diagnostics: ["claudeGateway", "diagnostics"] as const,
};

export function useClaudeAdapterState() {
  return useQuery({
    queryKey: claudeGatewayKeys.adapters,
    queryFn: claudeGatewayApi.getAdapterState,
    refetchInterval: 3000,
  });
}

function useAdapterMutation(
  mutationFn: (
    enabled: boolean,
  ) => ReturnType<typeof claudeGatewayApi.setClaudeCodeEnabled>,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn,
    onSuccess: (state) => {
      queryClient.setQueryData(claudeGatewayKeys.adapters, state);
      void queryClient.invalidateQueries({ queryKey: agentGatewayKeys.state });
    },
  });
}

export function useSetClaudeCodeEnabled() {
  return useAdapterMutation(claudeGatewayApi.setClaudeCodeEnabled);
}

export function useSetClaudeDesktopEnabled() {
  return useAdapterMutation(claudeGatewayApi.setClaudeDesktopEnabled);
}

export function useGatewayDiagnostics() {
  return useQuery({
    queryKey: claudeGatewayKeys.diagnostics,
    queryFn: claudeGatewayApi.getDiagnostics,
    refetchInterval: 3000,
  });
}

export function useLegacyImport() {
  const queryClient = useQueryClient();
  const preview = useMutation({
    mutationFn: claudeGatewayApi.previewLegacyImport,
  });
  const importDatabase = useMutation({
    mutationFn: ({
      filePath,
      replaceCanonicalProfile,
    }: {
      filePath: string;
      replaceCanonicalProfile: boolean;
    }) =>
      claudeGatewayApi.importLegacyDatabase(filePath, {
        replaceCanonicalProfile,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: claudeConnectionProfileKeys.state,
      });
      void queryClient.invalidateQueries({ queryKey: ["providers"] });
      void queryClient.invalidateQueries({ queryKey: ["mcp"] });
      void queryClient.invalidateQueries({ queryKey: ["skills"] });
      void queryClient.invalidateQueries({ queryKey: ["prompts"] });
      void queryClient.invalidateQueries({ queryKey: ["profiles"] });
    },
  });

  return { preview, importDatabase };
}

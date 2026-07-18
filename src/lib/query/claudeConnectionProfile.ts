import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { claudeConnectionProfileApi } from "@/lib/api/claudeConnectionProfile";
import type {
  ClaudeConnectionProfileInput,
  ClaudeConnectionProfileState,
  ClaudeConnectionTestInput,
} from "@/types/claudeConnectionProfile";

export const claudeConnectionProfileKeys = {
  state: ["claudeConnectionProfile", "state"] as const,
};

export function useClaudeConnectionProfile() {
  return useQuery({
    queryKey: claudeConnectionProfileKeys.state,
    queryFn: claudeConnectionProfileApi.get,
  });
}

export function useUpdateClaudeConnectionProfile() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: ClaudeConnectionProfileInput) =>
      claudeConnectionProfileApi.update(input),
    onSuccess: (state) => {
      queryClient.setQueryData<ClaudeConnectionProfileState>(
        claudeConnectionProfileKeys.state,
        state,
      );
    },
  });
}

export function useTestClaudeConnectionModel() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: ClaudeConnectionTestInput) =>
      claudeConnectionProfileApi.testOne(input),
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: claudeConnectionProfileKeys.state,
      }),
  });
}

export function useTestAllClaudeConnectionModels() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: claudeConnectionProfileApi.testAll,
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: claudeConnectionProfileKeys.state,
      }),
  });
}

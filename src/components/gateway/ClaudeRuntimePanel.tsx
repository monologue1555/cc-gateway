import type { LucideIcon } from "lucide-react";
import {
  AlertTriangle,
  Bot,
  Loader2,
  Monitor,
  RefreshCw,
  TerminalSquare,
} from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  agentGatewayKeys,
  useAgentGatewayState,
  useUpdateAgentGatewayConfig,
} from "@/lib/query/agentGateway";
import {
  claudeGatewayKeys,
  useClaudeAdapterState,
  useSetClaudeCodeEnabled,
  useSetClaudeDesktopEnabled,
} from "@/lib/query/claudeGateway";
import { cn } from "@/lib/utils";
import type { ClaudeGatewayPhase } from "@/types/claudeGateway";
import { extractErrorMessage } from "@/utils/errorUtils";

const PHASE_LABELS: Record<ClaudeGatewayPhase, string> = {
  disabled: "已停止",
  starting: "启动中",
  ready: "就绪",
  degraded: "异常",
  stopping: "停止中",
};

const PHASE_STYLES: Record<ClaudeGatewayPhase, string> = {
  disabled: "text-muted-foreground",
  starting: "text-amber-500",
  ready: "text-emerald-500",
  degraded: "text-red-500",
  stopping: "text-amber-500",
};

interface ConsumerCardProps {
  title: string;
  description: string;
  icon: LucideIcon;
  enabled: boolean;
  pending: boolean;
  disabled?: boolean;
  onToggle: (enabled: boolean) => void;
}

function ConsumerCard({
  title,
  description,
  icon: Icon,
  enabled,
  pending,
  disabled,
  onToggle,
}: ConsumerCardProps) {
  return (
    <div className="flex min-h-28 items-start justify-between gap-3 rounded-xl border bg-card/50 p-4">
      <div className="flex min-w-0 gap-3">
        <div
          className={cn(
            "flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border bg-background",
            enabled && "border-emerald-500/40 bg-emerald-500/5",
          )}
        >
          <Icon
            className={cn(
              "h-4 w-4 text-muted-foreground",
              enabled && "text-emerald-500",
            )}
          />
        </div>
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <p className="text-sm font-semibold">{title}</p>
            {enabled && <Badge variant="secondary">已接入</Badge>}
          </div>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            {description}
          </p>
        </div>
      </div>
      {pending ? (
        <Loader2 className="mt-1 h-4 w-4 shrink-0 animate-spin text-muted-foreground" />
      ) : (
        <Switch
          checked={enabled}
          onCheckedChange={onToggle}
          disabled={disabled}
          aria-label={`${title}${enabled ? "停用" : "启用"}`}
        />
      )}
    </div>
  );
}

export function ClaudeRuntimePanel() {
  const queryClient = useQueryClient();
  const adapterState = useClaudeAdapterState();
  const agentGatewayState = useAgentGatewayState();
  const setCodeEnabled = useSetClaudeCodeEnabled();
  const setDesktopEnabled = useSetClaudeDesktopEnabled();
  const updateBackend = useUpdateAgentGatewayConfig();

  const state = adapterState.data;
  const backendState = agentGatewayState.data;

  const toggle = async (label: string, mutation: () => Promise<unknown>) => {
    try {
      await mutation();
      await queryClient.invalidateQueries({
        queryKey: claudeGatewayKeys.adapters,
      });
      toast.success(`${label} 状态已更新`);
    } catch (error) {
      toast.error(`${label} 切换失败：${extractErrorMessage(error)}`);
    }
  };

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: claudeGatewayKeys.adapters }),
      queryClient.invalidateQueries({ queryKey: agentGatewayKeys.state }),
    ]);
  };

  if (adapterState.error) {
    return (
      <Alert variant="destructive">
        <AlertTriangle className="h-4 w-4" />
        <AlertDescription>
          无法读取本地路由状态：{extractErrorMessage(adapterState.error)}
        </AlertDescription>
      </Alert>
    );
  }

  if (adapterState.isLoading || !state) {
    return (
      <div className="flex justify-center py-10">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <section className="space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <h3 className="font-semibold">本地路由消费者</h3>
            <Badge
              variant="outline"
              className={PHASE_STYLES[state.runtime.phase]}
            >
              {PHASE_LABELS[state.runtime.phase]}
            </Badge>
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            三个入口共享 localhost:15722；最后一个入口停用后 listener 才会停止。
          </p>
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => void refresh()}
          disabled={adapterState.isFetching || agentGatewayState.isFetching}
        >
          <RefreshCw
            className={cn(
              "mr-2 h-4 w-4",
              (adapterState.isFetching || agentGatewayState.isFetching) &&
                "animate-spin",
            )}
          />
          刷新状态
        </Button>
      </div>

      <div className="grid gap-3 lg:grid-cols-3">
        <ConsumerCard
          title="Claude Code"
          description="写入本地 Gateway URL、本地 Key 与 canonical 角色模型映射。"
          icon={TerminalSquare}
          enabled={state.claudeCodeEnabled}
          pending={setCodeEnabled.isPending}
          onToggle={(enabled) =>
            void toggle("Claude Code", () =>
              setCodeEnabled.mutateAsync(enabled),
            )
          }
        />
        <ConsumerCard
          title="Claude Desktop"
          description="通过独立本地 Key 连接 /claude-desktop，不暴露 AnyRouter Key。"
          icon={Monitor}
          enabled={state.claudeDesktopEnabled}
          pending={setDesktopEnabled.isPending}
          onToggle={(enabled) =>
            void toggle("Claude Desktop", () =>
              setDesktopEnabled.mutateAsync(enabled),
            )
          }
        />
        <ConsumerCard
          title="Backend Gateway"
          description="向其他工具提供 Anthropic、Responses 与 Chat 三种本地入口。"
          icon={Bot}
          enabled={state.backendEnabled}
          pending={updateBackend.isPending}
          disabled={!backendState || agentGatewayState.isLoading}
          onToggle={(enabled) =>
            void toggle("Backend Gateway", () =>
              updateBackend.mutateAsync({
                enabled,
                emulateClaudeCode: backendState?.emulateClaudeCode ?? false,
              }),
            )
          }
        />
      </div>

      {state.runtime.lastError && (
        <Alert variant="destructive">
          <AlertTriangle className="h-4 w-4" />
          <AlertDescription>
            Runtime 最近错误：{state.runtime.lastError}
          </AlertDescription>
        </Alert>
      )}
    </section>
  );
}

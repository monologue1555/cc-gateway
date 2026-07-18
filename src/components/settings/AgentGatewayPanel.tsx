import { useEffect, useState } from "react";
import {
  Activity,
  CheckCircle2,
  Clipboard,
  Copy,
  KeyRound,
  Loader2,
  Play,
  RefreshCw,
  Server,
  XCircle,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { toast } from "sonner";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { copyText } from "@/lib/clipboard";
import {
  useAgentGatewayState,
  useRegenerateAgentGatewayToken,
  useTestAgentGatewayProtocol,
  useUpdateAgentGatewayConfig,
} from "@/lib/query/agentGateway";
import type {
  AgentGatewayConfigInput,
  AgentGatewayProtocol,
  AgentGatewayState,
  AgentGatewayTestResult,
} from "@/types/agentGateway";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";

const PROTOCOLS: AgentGatewayProtocol[] = ["anthropic", "responses", "chat"];

function configFromState(
  state: AgentGatewayState,
  updates: Partial<AgentGatewayConfigInput>,
): AgentGatewayConfigInput {
  return {
    enabled: state.enabled,
    emulateClaudeCode: state.emulateClaudeCode ?? false,
    ...updates,
  };
}

function createSnippet(
  protocol: AgentGatewayProtocol,
  endpoint: string,
  token: string | null,
  model: string,
): string {
  const apiKey = token || "<YOUR_GATEWAY_KEY>";

  if (protocol === "anthropic") {
    return `curl ${JSON.stringify(endpoint)} \\
  -H ${JSON.stringify(`x-api-key: ${apiKey}`)} \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '${JSON.stringify({
    model,
    max_tokens: 64,
    messages: [{ role: "user", content: "Reply with OK" }],
  })}'`;
  }

  if (protocol === "responses") {
    return `curl ${JSON.stringify(endpoint)} \\
  -H ${JSON.stringify(`Authorization: Bearer ${apiKey}`)} \\
  -H "content-type: application/json" \\
  -d '${JSON.stringify({ model, input: "Reply with OK" })}'`;
  }

  return `curl ${JSON.stringify(endpoint)} \\
  -H ${JSON.stringify(`Authorization: Bearer ${apiKey}`)} \\
  -H "content-type: application/json" \\
  -d '${JSON.stringify({
    model,
    messages: [{ role: "user", content: "Reply with OK" }],
  })}'`;
}

function protocolLabel(protocol: AgentGatewayProtocol, t: TFunction): string {
  if (protocol === "anthropic") return "Anthropic Messages";
  if (protocol === "responses") return "OpenAI Responses";
  return t("agentGateway.protocol.chat", {
    defaultValue: "Chat Completions",
  });
}

export function AgentGatewayPanel() {
  const { t } = useTranslation();
  const {
    data: state,
    isLoading: isStateLoading,
    error: stateError,
  } = useAgentGatewayState();
  const updateConfig = useUpdateAgentGatewayConfig();
  const regenerateToken = useRegenerateAgentGatewayToken();
  const testProtocol = useTestAgentGatewayProtocol();

  const [selectedModel, setSelectedModel] = useState("");
  const [recentToken, setRecentToken] = useState<string | null>(null);
  const [showRegenerateConfirm, setShowRegenerateConfirm] = useState(false);
  const [lastTests, setLastTests] = useState<
    Partial<Record<AgentGatewayProtocol, AgentGatewayTestResult>>
  >({});

  useEffect(() => {
    if (!state?.models.length) {
      setSelectedModel("");
      return;
    }
    if (!state.models.some((model) => model.id === selectedModel)) {
      setSelectedModel(state.models[0].id);
    }
  }, [selectedModel, state?.models]);

  const update = async (updates: Partial<AgentGatewayConfigInput>) => {
    if (!state) return;
    try {
      await updateConfig.mutateAsync(configFromState(state, updates));
    } catch (error) {
      toast.error(
        t("agentGateway.toast.saveFailed", {
          defaultValue: "Agent Gateway 配置保存失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    }
  };

  const handleRegenerateToken = async () => {
    setShowRegenerateConfirm(false);
    try {
      const result = await regenerateToken.mutateAsync();
      setRecentToken(result.token);
      await copyText(result.token);
      toast.success(
        t("agentGateway.toast.tokenRegenerated", {
          defaultValue: "新 Gateway Key 已生成并复制，请立即更新其他 Agent",
        }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("agentGateway.toast.tokenFailed", {
          defaultValue: "Gateway Key 生成失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    }
  };

  const handleCopy = async (value: string, successMessage: string) => {
    try {
      await copyText(value);
      toast.success(successMessage, { closeButton: true });
    } catch (error) {
      toast.error(
        t("agentGateway.toast.copyFailed", {
          defaultValue: "复制失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    }
  };

  const handleTest = async (protocol: AgentGatewayProtocol) => {
    if (!selectedModel) return;
    try {
      const result = await testProtocol.mutateAsync({
        protocol,
        model: selectedModel,
      });
      setLastTests((previous) => ({ ...previous, [protocol]: result }));
      if (result.success) {
        toast.success(
          t("agentGateway.toast.testPassed", {
            defaultValue: "{{protocol}} 测试通过（{{latency}} ms）",
            protocol: protocolLabel(protocol, t),
            latency: result.latencyMs,
          }),
          { closeButton: true },
        );
      } else {
        toast.error(
          result.message ||
            t("agentGateway.toast.testFailed", {
              defaultValue: "协议测试失败",
            }),
        );
      }
    } catch (error) {
      toast.error(
        t("agentGateway.toast.testFailedDetail", {
          defaultValue: "协议测试失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    }
  };

  if (isStateLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (!state || stateError) {
    return (
      <Alert variant="destructive">
        <XCircle className="h-4 w-4" />
        <AlertDescription>
          {t("agentGateway.loadFailed", {
            defaultValue: "无法读取 Agent Gateway 配置：{{error}}",
            error: extractErrorMessage(stateError),
          })}
        </AlertDescription>
      </Alert>
    );
  }

  return (
    <div className="space-y-6">
      <section className="grid gap-3 sm:grid-cols-2">
        <div className="flex items-center justify-between rounded-xl border border-border bg-card/50 p-4">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-background ring-1 ring-border">
              <Activity
                className={cn(
                  "h-4 w-4",
                  state.enabled ? "text-emerald-500" : "text-muted-foreground",
                )}
              />
            </div>
            <div className="space-y-1">
              <p className="text-sm font-medium leading-none">
                {t("agentGateway.service", { defaultValue: "Gateway 服务" })}
              </p>
              <p className="text-xs text-muted-foreground">
                {state.enabled
                  ? t("agentGateway.running", {
                      defaultValue: "Agent 入口已启用",
                    })
                  : t("agentGateway.stopped", {
                      defaultValue: "Agent 入口已停用",
                    })}
              </p>
            </div>
          </div>
          <Switch
            checked={state.enabled}
            onCheckedChange={(enabled) => update({ enabled })}
            disabled={
              updateConfig.isPending || (!state.enabled && !state.compatible)
            }
            title={
              !state.enabled && !state.compatible
                ? t("agentGateway.compatibility.enableBlocked", {
                    defaultValue: "请先切换到 Anthropic Messages（原生）供应商",
                  })
                : undefined
            }
          />
        </div>

        <div className="rounded-xl border border-border bg-card/50 p-4">
          <div className="mb-2 flex items-center justify-between">
            <div className="flex items-center gap-2 text-sm font-medium">
              <Server className="h-4 w-4 text-cyan-500" />
              {t("agentGateway.localEndpoint", {
                defaultValue: "固定本地监听",
              })}
            </div>
            <Badge variant="secondary">localhost only</Badge>
          </div>
          <code className="block truncate rounded-md bg-muted/70 px-3 py-2 text-xs">
            {state.listenAddress}:{state.listenPort}
          </code>
        </div>
      </section>

      <Alert
        variant={state.compatible ? "default" : "destructive"}
        className={
          state.compatible
            ? "border-emerald-500/30 bg-emerald-500/5 text-emerald-700 dark:text-emerald-300"
            : undefined
        }
      >
        {state.compatible ? (
          <CheckCircle2 className="h-4 w-4" />
        ) : (
          <XCircle className="h-4 w-4" />
        )}
        <AlertDescription>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="font-medium">
              {state.compatible
                ? t("agentGateway.compatibility.compatible", {
                    defaultValue: "当前 Claude Desktop 路由兼容",
                  })
                : t("agentGateway.compatibility.incompatible", {
                    defaultValue: "当前 Claude Desktop 路由不兼容",
                  })}
            </span>
            <Badge variant="outline">Anthropic Messages · native</Badge>
          </div>
          <p className="mt-1 text-xs opacity-90">
            {state.compatible
              ? t("agentGateway.compatibility.supported", {
                  defaultValue:
                    "Agent Gateway 会直接复用这条原生 Anthropic 路由。",
                })
              : state.compatibilityMessage ||
                t("agentGateway.compatibility.unsupported", {
                  defaultValue:
                    "MVP 仅支持 API 格式为 Anthropic Messages（原生）的 Claude Desktop 供应商。",
                })}
          </p>
        </AlertDescription>
      </Alert>

      <section className="space-y-3 rounded-xl border border-border bg-card/30 p-4">
        <div className="flex items-start justify-between gap-4">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <KeyRound className="h-4 w-4 text-amber-500" />
              {t("agentGateway.token.title", { defaultValue: "Gateway Key" })}
            </div>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("agentGateway.token.description", {
                defaultValue:
                  "其他 Agent 只使用这个本地密钥，上游供应商密钥不会暴露。",
              })}
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setShowRegenerateConfirm(true)}
            disabled={regenerateToken.isPending}
          >
            {regenerateToken.isPending ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="mr-2 h-4 w-4" />
            )}
            {t("agentGateway.token.regenerate", { defaultValue: "重新生成" })}
          </Button>
        </div>
        <div className="flex gap-2">
          <Input
            readOnly
            value={state.maskedToken || "agt_gateway_••••••••••••"}
            className="font-mono text-xs"
          />
          <Button
            variant="outline"
            size="icon"
            disabled={!recentToken}
            title={
              recentToken
                ? t("common.copy")
                : t("agentGateway.token.copyUnavailable", {
                    defaultValue: "为安全起见，只能复制本次新生成的密钥",
                  })
            }
            onClick={() =>
              recentToken &&
              handleCopy(
                recentToken,
                t("agentGateway.toast.tokenCopied", {
                  defaultValue: "Gateway Key 已复制",
                }),
              )
            }
          >
            <Copy className="h-4 w-4" />
          </Button>
        </div>
        {!recentToken && (
          <p className="text-xs text-muted-foreground">
            {t("agentGateway.token.oneTimeHint", {
              defaultValue:
                "明文密钥只在重新生成后于本次页面会话中可复制；重新生成会使旧密钥立即失效。",
            })}
          </p>
        )}
      </section>

      <section className="space-y-4 rounded-xl border border-border bg-card/30 p-4">
        <div>
          <h4 className="text-sm font-semibold">
            {t("agentGateway.providers.title", {
              defaultValue: "跟随 Claude Desktop 的 Anthropic 原生路由",
            })}
          </h4>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("agentGateway.providers.description", {
              defaultValue:
                "Gateway 不维护第二套路由配置；MVP 仅跟随 Anthropic Messages（原生）供应商，不支持 Desktop 的 OpenAI 或 Gemini 格式。",
            })}
          </p>
        </div>

        <div className="grid gap-4 sm:grid-cols-2">
          <div className="rounded-lg border border-border/70 bg-muted/30 px-4 py-3">
            <div className="flex items-center justify-between gap-3">
              <p className="text-sm font-medium">
                {t("agentGateway.providers.current", {
                  defaultValue: "Claude Desktop 当前供应商",
                })}
              </p>
              <Badge variant="secondary">
                {t("agentGateway.providers.readOnly", {
                  defaultValue: "自动跟随",
                })}
              </Badge>
            </div>
            {state.currentProviderId ? (
              <div className="mt-2 min-w-0">
                <p className="truncate text-sm font-semibold">
                  {state.currentProviderName || state.currentProviderId}
                </p>
                {state.currentProviderName && (
                  <code className="mt-1 block truncate text-[11px] text-muted-foreground">
                    {state.currentProviderId}
                  </code>
                )}
              </div>
            ) : (
              <p className="mt-2 text-xs text-amber-600 dark:text-amber-400">
                {t("agentGateway.providers.empty", {
                  defaultValue:
                    "Claude Desktop 当前没有可用的本地路由，请先在 Claude 页启用一个供应商。",
                })}
              </p>
            )}
          </div>

          <div className="rounded-lg border border-border/70 bg-muted/30 px-4 py-3">
            <div>
              <div className="flex items-center justify-between gap-3">
                <p className="text-sm font-medium">
                  {t("agentGateway.providers.routePolicy", {
                    defaultValue: "路由策略",
                  })}
                </p>
                <Badge variant="outline">
                  {state.autoFailoverEnabled
                    ? t("agentGateway.providers.failoverOn", {
                        defaultValue: "跟随自动切换",
                      })
                    : t("agentGateway.providers.failoverOff", {
                        defaultValue: "跟随当前供应商",
                      })}
                </Badge>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                {t("agentGateway.providers.failover", {
                  defaultValue:
                    "由 Claude Desktop 路由统一决定，Agent Gateway 不单独修改。",
                })}
              </p>
            </div>
          </div>
        </div>

        <div className="flex items-center justify-between rounded-lg border border-amber-500/20 bg-amber-500/5 px-4 py-3">
          <div className="pr-4">
            <p className="text-sm font-medium">
              {t("agentGateway.providers.emulateClaudeCode", {
                defaultValue: "模拟 Claude Code 请求",
              })}
            </p>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("agentGateway.providers.emulateClaudeCodeHint", {
                defaultValue:
                  "仅用于需要 Claude Code 客户端指纹的中转站；普通 Anthropic API 请保持关闭。",
              })}
            </p>
          </div>
          <Switch
            checked={state.emulateClaudeCode ?? false}
            onCheckedChange={(emulateClaudeCode) =>
              update({ emulateClaudeCode })
            }
            disabled={updateConfig.isPending}
          />
        </div>
      </section>

      <section className="space-y-3 rounded-xl border border-border bg-card/30 p-4">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h4 className="text-sm font-semibold">
              {t("agentGateway.models.title", { defaultValue: "可用模型" })}
            </h4>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("agentGateway.models.description", {
                defaultValue:
                  "模型目录、别名与角色回落完全沿用 Claude Desktop 现有规则。",
              })}
            </p>
          </div>
          <Badge variant="secondary">
            {state.models.length}{" "}
            {t("agentGateway.models.count", { defaultValue: "个" })}
          </Badge>
        </div>
        {state.models.length ? (
          <>
            <div className="flex flex-wrap gap-2">
              {state.models.map((model) => (
                <div
                  key={model.id}
                  className="rounded-full border border-border bg-background/40 px-3 py-1.5 text-xs"
                >
                  {model.label || model.id}
                  {model.supports1m && (
                    <span className="ml-1 text-[10px] text-emerald-500">
                      1M
                    </span>
                  )}
                </div>
              ))}
            </div>
            <details className="text-xs text-muted-foreground">
              <summary className="cursor-pointer select-none hover:text-foreground">
                {t("agentGateway.models.details", {
                  defaultValue: "查看模型 ID 与上游映射",
                })}
              </summary>
              <div className="mt-2 overflow-hidden rounded-lg border border-border/70">
                {state.models.map((model) => (
                  <div
                    key={model.id}
                    className="grid grid-cols-2 gap-3 border-b border-border/60 px-3 py-2 last:border-b-0"
                  >
                    <code className="truncate text-foreground">{model.id}</code>
                    <code className="truncate">
                      {model.upstreamModel || model.id}
                    </code>
                  </div>
                ))}
              </div>
            </details>
          </>
        ) : (
          <div className="rounded-lg border border-dashed border-border px-4 py-6 text-center text-sm text-muted-foreground">
            {!state.compatible
              ? state.compatibilityMessage ||
                t("agentGateway.compatibility.unsupported", {
                  defaultValue:
                    "MVP 仅支持 API 格式为 Anthropic Messages（原生）的 Claude Desktop 供应商。",
                })
              : t("agentGateway.models.empty", {
                  defaultValue:
                    "当前路由没有可暴露的模型。请先配置供应商模型映射。",
                })}
          </div>
        )}
      </section>

      <section className="space-y-4 rounded-xl border border-border bg-card/30 p-4">
        <div>
          <h4 className="text-sm font-semibold">
            {t("agentGateway.clients.title", { defaultValue: "Agent 接入" })}
          </h4>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("agentGateway.clients.description", {
              defaultValue:
                "复制与你的 Agent 协议匹配的最小请求；测试会通过本地 Gateway 调用当前模型。",
            })}
          </p>
        </div>

        <div className="space-y-2">
          <Label>
            {t("agentGateway.clients.testModel", {
              defaultValue: "示例与测试模型",
            })}
          </Label>
          <Select
            value={selectedModel || undefined}
            onValueChange={setSelectedModel}
            disabled={!state.compatible || !state.models.length}
          >
            <SelectTrigger>
              <SelectValue
                placeholder={t("agentGateway.clients.selectModel", {
                  defaultValue: "选择模型",
                })}
              />
            </SelectTrigger>
            <SelectContent>
              {state.models.map((model) => (
                <SelectItem key={model.id} value={model.id}>
                  {model.label ? `${model.label} · ${model.id}` : model.id}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <Tabs defaultValue="anthropic" className="w-full">
          <TabsList className="grid w-full grid-cols-3">
            {PROTOCOLS.map((protocol) => (
              <TabsTrigger
                key={protocol}
                value={protocol}
                className="min-w-0 px-2 text-xs"
              >
                {protocol === "anthropic"
                  ? "Anthropic"
                  : protocol === "responses"
                    ? "Responses"
                    : "Chat"}
              </TabsTrigger>
            ))}
          </TabsList>
          {PROTOCOLS.map((protocol) => {
            const endpoint = state.endpoints[protocol];
            const snippet = createSnippet(
              protocol,
              endpoint,
              recentToken,
              selectedModel || "<MODEL_ID>",
            );
            const lastTest = lastTests[protocol];
            const isTesting =
              testProtocol.isPending &&
              testProtocol.variables?.protocol === protocol;

            return (
              <TabsContent
                key={protocol}
                value={protocol}
                className="space-y-3 pt-2"
              >
                <div className="flex items-center gap-2 rounded-lg border border-border/70 bg-muted/30 p-2">
                  <code className="min-w-0 flex-1 truncate px-1 text-xs">
                    {endpoint}
                  </code>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={() =>
                      handleCopy(
                        endpoint,
                        t("agentGateway.toast.endpointCopied", {
                          defaultValue: "端点已复制",
                        }),
                      )
                    }
                  >
                    <Copy className="h-4 w-4" />
                  </Button>
                </div>
                <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-border/70 bg-background/70 p-3 font-mono text-[11px] leading-relaxed text-muted-foreground">
                  {snippet}
                </pre>
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0 text-xs text-muted-foreground">
                    {lastTest && (
                      <span
                        className={cn(
                          "inline-flex items-center gap-1.5",
                          lastTest.success
                            ? "text-emerald-600 dark:text-emerald-400"
                            : "text-destructive",
                        )}
                      >
                        {lastTest.success ? (
                          <CheckCircle2 className="h-3.5 w-3.5" />
                        ) : (
                          <XCircle className="h-3.5 w-3.5" />
                        )}
                        {lastTest.success
                          ? `${lastTest.latencyMs} ms · ${lastTest.model}`
                          : lastTest.message ||
                            t("agentGateway.toast.testFailed", {
                              defaultValue: "协议测试失败",
                            })}
                      </span>
                    )}
                  </div>
                  <div className="flex shrink-0 gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() =>
                        handleCopy(
                          snippet,
                          t("agentGateway.toast.snippetCopied", {
                            defaultValue: "配置片段已复制",
                          }),
                        )
                      }
                    >
                      <Clipboard className="mr-2 h-4 w-4" />
                      {t("common.copy")}
                    </Button>
                    <Button
                      size="sm"
                      onClick={() => handleTest(protocol)}
                      disabled={
                        !state.enabled ||
                        !state.compatible ||
                        !state.currentProviderId ||
                        !selectedModel ||
                        testProtocol.isPending
                      }
                    >
                      {isTesting ? (
                        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      ) : (
                        <Play className="mr-2 h-4 w-4" />
                      )}
                      {t("agentGateway.clients.test", { defaultValue: "测试" })}
                    </Button>
                  </div>
                </div>
              </TabsContent>
            );
          })}
        </Tabs>
      </section>

      <ConfirmDialog
        isOpen={showRegenerateConfirm}
        title={t("agentGateway.token.confirmTitle", {
          defaultValue: "重新生成 Gateway Key？",
        })}
        message={t("agentGateway.token.confirmMessage", {
          defaultValue:
            "旧密钥会立即失效，已连接的其他 Agent 必须更新配置。新密钥只显示一次并会自动复制。",
        })}
        confirmText={t("agentGateway.token.confirmAction", {
          defaultValue: "生成并复制",
        })}
        onConfirm={handleRegenerateToken}
        onCancel={() => setShowRegenerateConfirm(false)}
      />
    </div>
  );
}

import { useEffect, useState } from "react";
import {
  CheckCircle2,
  FlaskConical,
  Loader2,
  Save,
  XCircle,
} from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  useClaudeConnectionProfile,
  useTestAllClaudeConnectionModels,
  useTestClaudeConnectionModel,
  useUpdateClaudeConnectionProfile,
} from "@/lib/query/claudeConnectionProfile";
import type {
  ClaudeConnectionProfileInput,
  ClaudeModelRoute,
  ClaudeModelRole,
} from "@/types/claudeConnectionProfile";
import { extractErrorMessage } from "@/utils/errorUtils";

const ROLE_LABELS: Record<ClaudeModelRole, string> = {
  opus: "Opus",
  fable: "Fable",
  sonnet: "Sonnet",
  haiku: "Haiku",
  subagent: "Subagent",
  fallback: "Fallback",
};

export function ClaudeConnectionProfilePanel() {
  const profile = useClaudeConnectionProfile();
  const updateProfile = useUpdateClaudeConnectionProfile();
  const testOne = useTestClaudeConnectionModel();
  const testAll = useTestAllClaudeConnectionModels();
  const [draft, setDraft] = useState<ClaudeConnectionProfileInput | null>(null);
  const [replacementKey, setReplacementKey] = useState("");

  useEffect(() => {
    if (!profile.data) return;
    setDraft({
      enabled: profile.data.enabled,
      baseUrl: profile.data.baseUrl,
      models: profile.data.models,
    });
  }, [profile.data]);

  if (profile.error) {
    return (
      <Alert variant="destructive">
        <XCircle className="h-4 w-4" />
        <AlertDescription>
          {extractErrorMessage(profile.error)}
        </AlertDescription>
      </Alert>
    );
  }

  if (profile.isLoading || !draft) {
    return (
      <div className="flex justify-center py-10">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const updateModel = (
    role: ClaudeModelRole,
    patch: Partial<ClaudeModelRoute>,
  ) => {
    setDraft((current) =>
      current
        ? {
            ...current,
            models: current.models.map((model) =>
              model.role === role ? { ...model, ...patch } : model,
            ),
          }
        : current,
    );
  };

  const save = async () => {
    try {
      const state = await updateProfile.mutateAsync({
        ...draft,
        apiKey: replacementKey.trim() || undefined,
      });
      setDraft({
        enabled: state.enabled,
        baseUrl: state.baseUrl,
        models: state.models,
      });
      setReplacementKey("");
      toast.success("AnyRouter 配置已保存");
    } catch (error) {
      toast.error(`保存失败：${extractErrorMessage(error)}`);
    }
  };

  const runOne = async (role: ClaudeModelRole) => {
    try {
      const result = await testOne.mutateAsync({ role });
      if (result.success) {
        toast.success(
          `${ROLE_LABELS[role]}：${result.upstreamModel} → ${result.responseModel || "未返回模型名"}`,
        );
      } else {
        toast.error(`${ROLE_LABELS[role]}：${result.message}`);
      }
    } catch (error) {
      toast.error(`测试失败：${extractErrorMessage(error)}`);
    }
  };

  const runAll = async () => {
    try {
      const result = await testAll.mutateAsync();
      if (result.success) {
        toast.success(`全部 ${result.results.length} 个实际上游模型测试通过`);
      } else {
        toast.error("部分模型不可用，请查看下方最近测试结果");
      }
    } catch (error) {
      toast.error(`测试失败：${extractErrorMessage(error)}`);
    }
  };

  return (
    <div className="space-y-5">
      <section className="space-y-4 rounded-xl border bg-card/40 p-4">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h3 className="font-semibold">AnyRouter Claude 连接</h3>
            <p className="mt-1 text-xs text-muted-foreground">
              Claude Code、Claude Desktop 与 Backend Gateway
              共享这一份连接和模型目录。
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Badge variant="outline">Anthropic · Bearer</Badge>
            <Switch
              checked={draft.enabled}
              onCheckedChange={(enabled) =>
                setDraft((current) =>
                  current ? { ...current, enabled } : current,
                )
              }
            />
          </div>
        </div>

        <div className="grid gap-4 md:grid-cols-2">
          <div className="space-y-2">
            <Label htmlFor="claude-profile-base-url">Base URL</Label>
            <Input
              id="claude-profile-base-url"
              value={draft.baseUrl}
              onChange={(event) =>
                setDraft((current) =>
                  current
                    ? { ...current, baseUrl: event.target.value }
                    : current,
                )
              }
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="claude-profile-key">
              API Key{" "}
              {profile.data?.maskedApiKey && `(${profile.data.maskedApiKey})`}
            </Label>
            <Input
              id="claude-profile-key"
              type="password"
              autoComplete="off"
              placeholder={
                profile.data?.hasApiKey
                  ? "留空以保留当前 Key"
                  : "输入 AnyRouter Key"
              }
              value={replacementKey}
              onChange={(event) => setReplacementKey(event.target.value)}
            />
          </div>
        </div>
      </section>

      <section className="overflow-hidden rounded-xl border bg-card/40">
        <div className="grid grid-cols-[110px_1fr_1fr_80px_72px] gap-2 border-b bg-muted/30 px-4 py-2 text-xs font-medium text-muted-foreground">
          <span>角色</span>
          <span>客户端模型</span>
          <span>实际上游模型</span>
          <span>1M</span>
          <span>测试</span>
        </div>
        {draft.models.map((model) => (
          <div
            key={model.role}
            className="grid grid-cols-[110px_1fr_1fr_80px_72px] items-center gap-2 border-b px-4 py-3 last:border-b-0"
          >
            <div>
              <p className="text-sm font-medium">{ROLE_LABELS[model.role]}</p>
              <Input
                className="mt-1 h-7 text-xs"
                value={model.displayName}
                onChange={(event) =>
                  updateModel(model.role, { displayName: event.target.value })
                }
              />
            </div>
            <Input
              className="font-mono text-xs"
              value={model.clientModelId}
              onChange={(event) =>
                updateModel(model.role, { clientModelId: event.target.value })
              }
            />
            <Input
              className="font-mono text-xs"
              value={model.upstreamModelId}
              onChange={(event) =>
                updateModel(model.role, { upstreamModelId: event.target.value })
              }
            />
            <Switch
              checked={model.supports1m}
              onCheckedChange={(supports1m) =>
                updateModel(model.role, { supports1m })
              }
            />
            <Button
              size="sm"
              variant="ghost"
              disabled={testOne.isPending}
              onClick={() => runOne(model.role)}
            >
              <FlaskConical className="h-4 w-4" />
            </Button>
          </div>
        ))}
      </section>

      {profile.data?.lastTest && (
        <Alert
          variant={profile.data.lastTest.success ? "default" : "destructive"}
        >
          {profile.data.lastTest.success ? (
            <CheckCircle2 className="h-4 w-4" />
          ) : (
            <XCircle className="h-4 w-4" />
          )}
          <AlertDescription className="space-y-1">
            {profile.data.lastTest.results.map((result) => (
              <div
                key={`${result.role}-${result.upstreamModel}`}
                className="text-xs"
              >
                {ROLE_LABELS[result.role]} · {result.upstreamModel} →{` `}
                {result.responseModel || result.category} · {result.latencyMs}{" "}
                ms
              </div>
            ))}
          </AlertDescription>
        </Alert>
      )}

      <div className="flex justify-end gap-2">
        <Button variant="outline" onClick={runAll} disabled={testAll.isPending}>
          {testAll.isPending ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <FlaskConical className="mr-2 h-4 w-4" />
          )}
          测试全部模型
        </Button>
        <Button onClick={save} disabled={updateProfile.isPending}>
          {updateProfile.isPending ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <Save className="mr-2 h-4 w-4" />
          )}
          保存配置
        </Button>
      </div>
    </div>
  );
}

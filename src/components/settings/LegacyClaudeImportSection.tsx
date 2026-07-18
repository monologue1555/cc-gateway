import { useMemo, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Database,
  FileSearch,
  Loader2,
  ShieldCheck,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { settingsApi } from "@/lib/api";
import { useLegacyImport } from "@/lib/query/claudeGateway";
import type {
  LegacyImportCounts,
  LegacyImportPreview,
} from "@/types/claudeGateway";
import { extractErrorMessage } from "@/utils/errorUtils";

interface LegacyClaudeImportSectionProps {
  onImportSuccess?: () => void | Promise<void>;
}

function totalProviders(counts: { claude: number; claudeDesktop: number }) {
  return counts.claude + counts.claudeDesktop;
}

function SummaryGrid({
  values,
}: {
  values: Pick<
    LegacyImportPreview | LegacyImportCounts,
    "providers" | "mcpServers" | "skills" | "prompts" | "profiles"
  >;
}) {
  const items = [
    ["Claude 连接", totalProviders(values.providers)],
    ["MCP", values.mcpServers],
    ["Skills", values.skills],
    ["Prompts", values.prompts],
    ["Profiles", values.profiles],
  ] as const;

  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
      {items.map(([label, count]) => (
        <div
          key={label}
          className="rounded-lg border bg-background/60 px-3 py-2"
        >
          <div className="text-lg font-semibold tabular-nums">{count}</div>
          <div className="text-[11px] text-muted-foreground">{label}</div>
        </div>
      ))}
    </div>
  );
}

export function LegacyClaudeImportSection({
  onImportSuccess,
}: LegacyClaudeImportSectionProps) {
  const { preview, importDatabase } = useLegacyImport();
  const [filePath, setFilePath] = useState("");
  const [replaceCanonicalProfile, setReplaceCanonicalProfile] = useState(false);

  const fileName = useMemo(
    () => filePath.split(/[\\/]/).pop() || filePath,
    [filePath],
  );

  const selectFile = async () => {
    try {
      const selected = await settingsApi.openFileDialog();
      if (!selected) return;
      setFilePath(selected);
      setReplaceCanonicalProfile(false);
      importDatabase.reset();
      await preview.mutateAsync(selected);
    } catch (error) {
      toast.error(`无法预览旧数据库：${extractErrorMessage(error)}`);
    }
  };

  const clear = () => {
    setFilePath("");
    setReplaceCanonicalProfile(false);
    preview.reset();
    importDatabase.reset();
  };

  const runImport = async () => {
    if (!filePath || !preview.data) return;
    try {
      const result = await importDatabase.mutateAsync({
        filePath,
        replaceCanonicalProfile,
      });
      await onImportSuccess?.();
      toast.success("Claude 数据已选择性导入；未写入任何 Live 配置");
      if (result.liveConfigurationTouched) {
        toast.error("后端报告触及了 Live 配置，请停止使用并检查日志");
      }
    } catch (error) {
      toast.error(`选择性导入失败：${extractErrorMessage(error)}`);
    }
  };

  return (
    <section className="space-y-4">
      <header>
        <div className="flex items-center gap-2">
          <Database className="h-5 w-5 text-blue-500" />
          <h3 className="font-semibold">从旧 CC Switch 选择性导入</h3>
        </div>
        <p className="mt-1 text-sm text-muted-foreground">
          只读扫描旧数据库，仅复制 Claude Code、Claude Desktop、模型映射以及
          Claude 的 MCP、Skills、Prompts；不会导入其他 Agent 或接管状态。
        </p>
      </header>

      <Alert>
        <ShieldCheck className="h-4 w-4" />
        <AlertDescription>
          导入只更新 CC Gateway 自己的数据库，不自动写入 Claude Code 或 Claude
          Desktop 的 Live 配置。
        </AlertDescription>
      </Alert>

      <div className="rounded-xl border bg-muted/20 p-4">
        <div className="flex flex-wrap items-center gap-3">
          <Button
            type="button"
            variant="outline"
            onClick={() => void selectFile()}
            disabled={preview.isPending || importDatabase.isPending}
          >
            {preview.isPending ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <FileSearch className="mr-2 h-4 w-4" />
            )}
            选择旧数据库并预览
          </Button>
          {filePath && (
            <div className="flex min-w-0 flex-1 items-center gap-2">
              <code className="min-w-0 flex-1 truncate rounded bg-background px-3 py-2 text-xs">
                {fileName}
              </code>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                onClick={clear}
                disabled={importDatabase.isPending}
                title="清除选择"
              >
                <X className="h-4 w-4" />
              </Button>
            </div>
          )}
        </div>

        {preview.error && (
          <Alert variant="destructive" className="mt-4">
            <AlertTriangle className="h-4 w-4" />
            <AlertDescription>
              {extractErrorMessage(preview.error)}
            </AlertDescription>
          </Alert>
        )}

        {preview.data && (
          <div className="mt-4 space-y-4">
            <SummaryGrid values={preview.data} />

            {preview.data.canonicalCandidate && (
              <div className="space-y-3 rounded-lg border bg-background/50 p-3">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <p className="text-sm font-medium">
                      发现 canonical 连接候选
                    </p>
                    <p className="mt-0.5 text-xs text-muted-foreground">
                      {preview.data.canonicalCandidate.sourceProviderName} ·{" "}
                      {preview.data.canonicalCandidate.baseUrl} ·{" "}
                      {preview.data.canonicalCandidate.maskedApiKey}
                    </p>
                  </div>
                  <Badge variant="outline">
                    {preview.data.canonicalCandidate.models.length} 个模型映射
                  </Badge>
                </div>
                <div className="flex items-start gap-2">
                  <Checkbox
                    id="replace-canonical-profile"
                    checked={replaceCanonicalProfile}
                    onCheckedChange={(checked) =>
                      setReplaceCanonicalProfile(checked === true)
                    }
                  />
                  <Label
                    htmlFor="replace-canonical-profile"
                    className="text-xs font-normal leading-relaxed"
                  >
                    若当前 CC Gateway
                    已配置连接，也用这个候选覆盖。默认关闭以保护现有 AnyRouter
                    配置。
                  </Label>
                </div>
              </div>
            )}

            {preview.data.warnings.length > 0 && (
              <Alert>
                <AlertTriangle className="h-4 w-4" />
                <AlertDescription className="space-y-1">
                  {preview.data.warnings.map((warning) => (
                    <p key={warning} className="text-xs">
                      {warning}
                    </p>
                  ))}
                </AlertDescription>
              </Alert>
            )}

            <div className="flex justify-end">
              <Button
                type="button"
                onClick={() => void runImport()}
                disabled={importDatabase.isPending}
              >
                {importDatabase.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Database className="mr-2 h-4 w-4" />
                )}
                仅导入预览中的 Claude 数据
              </Button>
            </div>
          </div>
        )}
      </div>

      {importDatabase.data && (
        <Alert>
          <CheckCircle2 className="h-4 w-4" />
          <AlertDescription className="space-y-2">
            <p className="font-medium">选择性导入完成</p>
            <SummaryGrid values={importDatabase.data.imported} />
            <p className="text-xs text-muted-foreground">
              canonical 连接：{importDatabase.data.canonicalProfile.reason}
            </p>
          </AlertDescription>
        </Alert>
      )}
    </section>
  );
}

import { AlertTriangle, ExternalLink, LogOut } from "lucide-react";
import { exit } from "@tauri-apps/plugin-process";
import { Button } from "@/components/ui/button";
import { settingsApi } from "@/lib/api/settings";

const RELEASES_URL = "https://github.com/monologue1555/cc-gateway/releases";

interface DatabaseUpgradeProps {
  payload: {
    path?: string;
    error?: string;
    db_version?: number;
    supported_version?: number;
  };
}

export function DatabaseUpgrade({ payload }: DatabaseUpgradeProps) {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-6 text-foreground">
      <div className="w-full max-w-lg space-y-5 rounded-xl border bg-card p-7 shadow-lg">
        <div className="flex items-start gap-4">
          <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg bg-amber-500/10 text-amber-600 dark:text-amber-400">
            <AlertTriangle className="h-5 w-5" />
          </div>
          <div>
            <h1 className="text-lg font-semibold">数据库版本高于当前应用</h1>
            <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
              请从 CC Gateway
              发布页安装兼容版本。应用不会自动降级数据库，也不会删除现有数据。
            </p>
          </div>
        </div>

        <div className="space-y-2 rounded-lg border bg-muted/40 p-3 text-xs text-muted-foreground">
          {payload.db_version != null && payload.supported_version != null && (
            <p className="tabular-nums">
              数据库版本 v{payload.db_version} · 当前支持 v
              {payload.supported_version}
            </p>
          )}
          {payload.path && <p className="break-all">数据库：{payload.path}</p>}
          {payload.error && (
            <p className="break-words font-mono">{payload.error}</p>
          )}
        </div>

        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="outline" onClick={() => void exit(0)}>
            <LogOut className="mr-2 h-4 w-4" />
            退出
          </Button>
          <Button onClick={() => void settingsApi.openExternal(RELEASES_URL)}>
            <ExternalLink className="mr-2 h-4 w-4" />
            打开发布页
          </Button>
        </div>
      </div>
    </div>
  );
}

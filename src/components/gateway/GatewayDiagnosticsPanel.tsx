import { Activity, AlertTriangle, Loader2, RefreshCw } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useGatewayDiagnostics } from "@/lib/query/claudeGateway";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";

function formatTimestamp(timestamp: string): string {
  const value = new Date(timestamp);
  return Number.isNaN(value.getTime()) ? timestamp : value.toLocaleTimeString();
}

export function GatewayDiagnosticsPanel() {
  const diagnostics = useGatewayDiagnostics();
  const entries = [...(diagnostics.data ?? [])].reverse();

  return (
    <section className="space-y-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <Activity className="h-4 w-4 text-cyan-500" />
            <h3 className="font-semibold">最近诊断</h3>
            <Badge variant="secondary">{entries.length} / 50</Badge>
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            仅保留请求 ID、协议、模型、状态和耗时；不记录
            prompt、正文、认证头或密钥。
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => void diagnostics.refetch()}
          disabled={diagnostics.isFetching}
          title="刷新诊断"
        >
          <RefreshCw
            className={cn("h-4 w-4", diagnostics.isFetching && "animate-spin")}
          />
        </Button>
      </div>

      {diagnostics.error ? (
        <Alert variant="destructive">
          <AlertTriangle className="h-4 w-4" />
          <AlertDescription>
            无法读取诊断：{extractErrorMessage(diagnostics.error)}
          </AlertDescription>
        </Alert>
      ) : diagnostics.isLoading ? (
        <div className="flex justify-center py-8">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      ) : entries.length === 0 ? (
        <div className="rounded-xl border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
          暂无请求。启用任一入口并发起请求后，这里会显示脱敏诊断。
        </div>
      ) : (
        <div className="max-h-80 overflow-auto rounded-xl border">
          <Table>
            <TableHeader className="sticky top-0 bg-background">
              <TableRow>
                <TableHead>时间 / 请求</TableHead>
                <TableHead>协议</TableHead>
                <TableHead>请求模型 → 上游模型</TableHead>
                <TableHead>供应商</TableHead>
                <TableHead>状态</TableHead>
                <TableHead className="text-right">首字节</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {entries.map((entry) => (
                <TableRow key={`${entry.requestId}-${entry.timestamp}`}>
                  <TableCell>
                    <div className="text-xs">
                      {formatTimestamp(entry.timestamp)}
                    </div>
                    <code className="text-[10px] text-muted-foreground">
                      {entry.requestId}
                    </code>
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline">{entry.protocol}</Badge>
                  </TableCell>
                  <TableCell className="max-w-64">
                    <div className="truncate font-mono text-xs">
                      {entry.requestedModel}
                    </div>
                    <div className="truncate font-mono text-[11px] text-muted-foreground">
                      → {entry.upstreamModel}
                    </div>
                  </TableCell>
                  <TableCell className="text-xs">{entry.provider}</TableCell>
                  <TableCell>
                    <Badge
                      variant={
                        entry.status === "success" ? "secondary" : "destructive"
                      }
                    >
                      {entry.status}
                    </Badge>
                    <div className="mt-1 text-[10px] text-muted-foreground">
                      {entry.endReason}
                    </div>
                  </TableCell>
                  <TableCell className="text-right text-xs tabular-nums">
                    {entry.firstByteMs == null
                      ? "—"
                      : `${entry.firstByteMs} ms`}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </section>
  );
}

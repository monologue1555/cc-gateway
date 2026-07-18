import { ArchiveRestore, DatabaseZap, Route, Waypoints } from "lucide-react";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { Badge } from "@/components/ui/badge";
import { ClaudeConnectionProfilePanel } from "@/components/settings/ClaudeConnectionProfilePanel";
import { AgentGatewayPanel } from "@/components/settings/AgentGatewayPanel";
import { ClaudeRuntimePanel } from "@/components/gateway/ClaudeRuntimePanel";
import { GatewayDiagnosticsPanel } from "@/components/gateway/GatewayDiagnosticsPanel";
import { LegacyClaudeImportSection } from "@/components/settings/LegacyClaudeImportSection";

export function ClaudeGatewayDashboard() {
  return (
    <div className="mx-auto w-full max-w-7xl space-y-5 px-6 pb-12">
      <section className="rounded-2xl border bg-gradient-to-br from-cyan-500/10 via-background to-violet-500/10 p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="flex gap-3">
            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border bg-background/80">
              <Route className="h-5 w-5 text-cyan-500" />
            </div>
            <div>
              <h2 className="text-lg font-semibold">AnyRouter Claude 控制台</h2>
              <p className="mt-1 max-w-3xl text-sm text-muted-foreground">
                一份上游配置同时服务 Claude Code、Claude Desktop 与本地
                Backend；客户端只接触独立的本地 Key。
              </p>
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <Badge variant="outline">Anthropic Messages</Badge>
            <Badge variant="outline">localhost:15722</Badge>
          </div>
        </div>
      </section>

      <section className="rounded-2xl border bg-card/30 p-5">
        <ClaudeRuntimePanel />
      </section>

      <Accordion
        type="multiple"
        defaultValue={["connection", "diagnostics"]}
        className="space-y-4"
      >
        <AccordionItem
          value="connection"
          className="overflow-hidden rounded-2xl border bg-card/30"
        >
          <AccordionTrigger className="px-5 py-4 hover:no-underline">
            <div className="flex items-center gap-3 text-left">
              <DatabaseZap className="h-5 w-5 text-violet-500" />
              <div>
                <h3 className="font-semibold">AnyRouter 与模型目录</h3>
                <p className="mt-0.5 text-xs font-normal text-muted-foreground">
                  canonical 配置；模型请求发往上游前会自动移除 [1M] 声明。
                </p>
              </div>
            </div>
          </AccordionTrigger>
          <AccordionContent className="border-t px-5 pb-5 pt-4">
            <ClaudeConnectionProfilePanel />
          </AccordionContent>
        </AccordionItem>

        <AccordionItem
          value="backend"
          className="overflow-hidden rounded-2xl border bg-card/30"
        >
          <AccordionTrigger className="px-5 py-4 hover:no-underline">
            <div className="flex items-center gap-3 text-left">
              <Waypoints className="h-5 w-5 text-cyan-500" />
              <div>
                <h3 className="font-semibold">Backend 接入与协议测试</h3>
                <p className="mt-0.5 text-xs font-normal text-muted-foreground">
                  Anthropic、Responses、Chat 和模型目录共用独立 Gateway Key。
                </p>
              </div>
            </div>
          </AccordionTrigger>
          <AccordionContent className="border-t px-5 pb-5 pt-4">
            <AgentGatewayPanel />
          </AccordionContent>
        </AccordionItem>

        <AccordionItem
          value="diagnostics"
          className="overflow-hidden rounded-2xl border bg-card/30"
        >
          <AccordionTrigger className="px-5 py-4 hover:no-underline">
            <div className="flex items-center gap-3 text-left">
              <Route className="h-5 w-5 text-emerald-500" />
              <div>
                <h3 className="font-semibold">请求诊断</h3>
                <p className="mt-0.5 text-xs font-normal text-muted-foreground">
                  最近 50 条内存记录；提前断开的流式请求显示 stream_incomplete。
                </p>
              </div>
            </div>
          </AccordionTrigger>
          <AccordionContent className="border-t px-5 pb-5 pt-4">
            <GatewayDiagnosticsPanel />
          </AccordionContent>
        </AccordionItem>

        <AccordionItem
          value="legacy-import"
          className="overflow-hidden rounded-2xl border bg-card/30"
        >
          <AccordionTrigger className="px-5 py-4 hover:no-underline">
            <div className="flex items-center gap-3 text-left">
              <ArchiveRestore className="h-5 w-5 text-amber-500" />
              <div>
                <h3 className="font-semibold">选择性导入</h3>
                <p className="mt-0.5 text-xs font-normal text-muted-foreground">
                  只读取旧版 Claude 数据，不接管现有 Live 配置或同步状态。
                </p>
              </div>
            </div>
          </AccordionTrigger>
          <AccordionContent className="border-t px-5 pb-5 pt-4">
            <LegacyClaudeImportSection />
          </AccordionContent>
        </AccordionItem>
      </Accordion>

      <footer className="pb-3 text-center text-xs text-muted-foreground">
        CC Gateway 0.1.0 · 本地路由仅监听回环地址
      </footer>
    </div>
  );
}

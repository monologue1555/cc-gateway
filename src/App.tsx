import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Maximize2,
  Minimize2,
  Minus,
  Route,
  ShieldCheck,
  X,
} from "lucide-react";
import { ClaudeGatewayDashboard } from "@/components/gateway/ClaudeGatewayDashboard";
import { Button } from "@/components/ui/button";
import {
  DRAG_REGION_ATTR,
  DRAG_REGION_STYLE,
  isLinux,
  isWindows,
} from "@/lib/platform";

const CUSTOM_WINDOW_CONTROLS = isWindows() || isLinux();
const TITLE_BAR_HEIGHT = CUSTOM_WINDOW_CONTROLS ? 36 : 28;
const HEADER_HEIGHT = 58;

function App() {
  const [isWindowMaximized, setIsWindowMaximized] = useState(false);

  useEffect(() => {
    if (!CUSTOM_WINDOW_CONTROLS) return;

    let active = true;
    let unlistenResize: (() => void) | undefined;
    const currentWindow = getCurrentWindow();

    const syncMaximizedState = async () => {
      const maximized = await currentWindow.isMaximized();
      if (active) setIsWindowMaximized(maximized);
    };

    void (async () => {
      try {
        await syncMaximizedState();
        unlistenResize = await currentWindow.onResized(() => {
          void syncMaximizedState();
        });
      } catch (error) {
        console.error("[CC Gateway] Failed to sync window state", error);
      }
    })();

    return () => {
      active = false;
      unlistenResize?.();
    };
  }, []);

  const currentWindow = () => getCurrentWindow();

  return (
    <div className="min-h-screen bg-background text-foreground">
      <div
        className="fixed inset-x-0 top-0 z-[60] flex items-center justify-end border-b bg-background/95 px-2 backdrop-blur"
        {...DRAG_REGION_ATTR}
        style={
          {
            ...DRAG_REGION_STYLE,
            height: TITLE_BAR_HEIGHT,
          } as React.CSSProperties
        }
      >
        {CUSTOM_WINDOW_CONTROLS && (
          <div
            className="flex items-center"
            style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
          >
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-8 rounded-md"
              title="最小化"
              onClick={() => void currentWindow().minimize()}
            >
              <Minus className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-8 rounded-md"
              title={isWindowMaximized ? "还原" : "最大化"}
              onClick={() => void currentWindow().toggleMaximize()}
            >
              {isWindowMaximized ? (
                <Minimize2 className="h-4 w-4" />
              ) : (
                <Maximize2 className="h-4 w-4" />
              )}
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-8 rounded-md hover:bg-red-500/15 hover:text-red-500"
              title="关闭"
              onClick={() => void currentWindow().close()}
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
        )}
      </div>

      <header
        className="fixed inset-x-0 z-50 border-b bg-background/90 backdrop-blur"
        {...DRAG_REGION_ATTR}
        style={
          {
            ...DRAG_REGION_STYLE,
            top: TITLE_BAR_HEIGHT,
            height: HEADER_HEIGHT,
          } as React.CSSProperties
        }
      >
        <div className="mx-auto flex h-full max-w-7xl items-center justify-between gap-4 px-6">
          <div className="flex items-center gap-3">
            <div className="flex h-9 w-9 items-center justify-center rounded-lg border bg-card">
              <Route className="h-5 w-5 text-cyan-500" />
            </div>
            <div>
              <h1 className="text-base font-semibold">CC Gateway</h1>
              <p className="text-xs text-muted-foreground">
                Claude Code · Claude Desktop · Backend
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <ShieldCheck className="h-4 w-4 text-emerald-500" />
            <span>本地 Key 隔离</span>
          </div>
        </div>
      </header>

      <main
        style={{ paddingTop: TITLE_BAR_HEIGHT + HEADER_HEIGHT + 20 }}
        className="min-h-screen"
      >
        <ClaudeGatewayDashboard />
      </main>
    </div>
  );
}

export default App;

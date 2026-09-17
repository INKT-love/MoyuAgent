import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-solid";
import { desktop } from "../lib/api";

export default function WindowControls() {
  if (!desktop) return null;
  const appWindow = getCurrentWindow();
  const [maximized, setMaximized] = createSignal(false);
  onMount(() => {
    let disposed = false;
    const sync = () => {
      void appWindow.isMaximized().then((value) => {
        if (!disposed) setMaximized(value);
      });
    };
    sync();
    const unlisten = appWindow.onResized(sync);
    onCleanup(() => {
      disposed = true;
      void unlisten.then((stop) => stop());
    });
  });
  return (
    <div class="window-controls" data-tauri-drag-region="false">
      <button
        type="button"
        class="window-control"
        title="最小化"
        aria-label="最小化"
        onClick={() => void appWindow.minimize()}
      >
        <Minus size={14} />
      </button>
      <button
        type="button"
        class="window-control"
        title={maximized() ? "还原" : "最大化"}
        aria-label={maximized() ? "还原" : "最大化"}
        onClick={() => void appWindow.toggleMaximize()}
      >
        <Show when={maximized()} fallback={<Square size={11} />}>
          <Copy size={11} />
        </Show>
      </button>
      <button
        type="button"
        class="window-control window-close"
        title="关闭"
        aria-label="关闭窗口"
        onClick={() => void appWindow.close()}
      >
        <X size={14} />
      </button>
    </div>
  );
}

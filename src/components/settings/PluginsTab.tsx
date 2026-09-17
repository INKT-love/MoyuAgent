import { createSignal, For, onMount, Show } from "solid-js";
import { Puzzle, Plus, Trash2 } from "lucide-solid";
import type { PluginInfo } from "../../lib/api";
import { command, desktop, errorMessage } from "../../lib/api";

export function PluginsTab(props: {
  streaming: boolean;
  onChange?: () => void;
}) {
  const [plugins, setPlugins] = createSignal<PluginInfo[]>([]);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");

  const refresh = async () => {
    setPlugins(await command<PluginInfo[]>("list_plugins"));
  };

  onMount(() => {
    void refresh().catch((err) => setError(errorMessage(err)));
  });

  const run = async (work: () => Promise<PluginInfo[]>) => {
    if (props.streaming || busy()) return;
    setBusy(true);
    setError("");
    try {
      setPlugins(await work());
      props.onChange?.();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      class="settings-card"
      role="tabpanel"
      id="settings-panel-plugins"
      aria-labelledby="settings-tab-plugins"
    >
      <div class="section-heading">
        <Puzzle size={18} />
        <h2>Harness 插件</h2>
        <button
          type="button"
          class="text-button section-action"
          disabled={!desktop || props.streaming || busy()}
          onClick={() => void run(() => command<PluginInfo[]>("install_plugin"))}
        >
          <Plus size={14} />
          安装插件
        </button>
      </div>
      <p class="settings-copy">
        写插件只需含 plugin.json 的目录。界面用 CSS select 插入。
        后端放 backend.wasm，导出 before，对着任意 Tauri 命令名注入，不必在 Agent 里埋钩子。
        启用「WASM探针」后，设置里会多一页，输入栏会出现标记。
      </p>
      <Show when={error()}>
        <div role="alert" class="notice error-notice">
          {error()}
        </div>
      </Show>
      <Show
        when={plugins().length}
        fallback={<p class="settings-copy">还没有插件。选择一个含 plugin.json 的目录安装。</p>}
      >
        <ul class="plugin-list">
          <For each={plugins()}>
            {(plugin) => (
              <li class="plugin-row">
                <label class="plugin-toggle">
                  <input
                    type="checkbox"
                    checked={plugin.enabled}
                    disabled={!desktop || props.streaming || busy()}
                    onChange={(event) =>
                      void run(() =>
                        command<PluginInfo[]>("set_plugin_enabled", {
                          id: plugin.id,
                          enabled: event.currentTarget.checked,
                        }),
                      )
                    }
                  />
                  <span>
                    <strong>{plugin.name}</strong>
                    <small>
                      {plugin.version || "0.1.0"}
                      {plugin.bundled ? " · 内置" : ""}
                      {plugin.hasModule ? " · 模块" : ""}
                      {plugin.hasUi ? " · 界面" : ""}
                    </small>
                    <em>{plugin.description || "无说明"}</em>
                  </span>
                </label>
                <Show when={!plugin.bundled}>
                  <button
                    type="button"
                    class="icon-button"
                    title="卸载插件"
                    aria-label={`卸载 ${plugin.name}`}
                    disabled={!desktop || props.streaming || busy()}
                    onClick={() =>
                      void run(() =>
                        command<PluginInfo[]>("uninstall_plugin", {
                          id: plugin.id,
                        }),
                      )
                    }
                  >
                    <Trash2 size={14} />
                  </button>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </section>
  );
}

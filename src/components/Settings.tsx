import { For, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { AppWindow, Globe2, KeyRound, Puzzle } from "lucide-solid";
import type { AppState, ConfigSummary, Group } from "../lib/api";
import { folderName } from "../lib/api";
import { withBridge, type UiInjection } from "../lib/mixin";
import { EndpointSelect } from "./settings/EndpointSelect";
import { EndpointTab } from "./settings/EndpointTab";
import { ModelTab } from "./settings/ModelTab";
import { PluginsTab } from "./settings/PluginsTab";

export { EndpointSelect };
export type BuiltinSettingsTab = "endpoint" | "model" | "plugins";
export type SettingsTab = BuiltinSettingsTab | (string & {});

export function pluginSettingsTab(id: string) {
  return `plugin-${id}`;
}

const TABS: {
  id: BuiltinSettingsTab;
  label: string;
  icon: typeof Globe2;
}[] = [
  { id: "endpoint", label: "线路", icon: Globe2 },
  { id: "model", label: "模型", icon: KeyRound },
  { id: "plugins", label: "插件", icon: Puzzle },
];

const DESCRIPTIONS: Record<BuiltinSettingsTab, string> = {
  endpoint: "选择 API 线路",
  model: "选择分组与可用模型",
  plugins: "安装并组合 Harness 插件",
};

const isBuiltinTab = (tab: string): tab is BuiltinSettingsTab =>
  tab === "endpoint" || tab === "model" || tab === "plugins";

interface Props {
  state: AppState;
  groups: Group[];
  busy: boolean;
  streaming: boolean;
  tab: SettingsTab;
  pages: UiInjection[];
  onTab: (tab: SettingsTab) => void;
  onEndpoint: (index: number) => Promise<void>;
  onConfigured: (config: ConfigSummary) => void;
  onRefreshGroups: () => Promise<void>;
  onPluginsChange?: () => void;
}

export default function Settings(props: Props) {
  const tabs = () => [
    ...TABS,
    ...props.pages.map((page) => ({
      id: pluginSettingsTab(page.id) as SettingsTab,
      label: page.name,
      icon: AppWindow,
    })),
  ];
  const description = () =>
    isBuiltinTab(props.tab)
      ? DESCRIPTIONS[props.tab]
      : "插件混入的设置页";
  const moveTab = (offset: number) => {
    const list = tabs();
    const index = list.findIndex((tab) => tab.id === props.tab);
    const current = index < 0 ? 0 : index;
    const next = list[(current + offset + list.length) % list.length];
    props.onTab(next.id);
    requestAnimationFrame(() =>
      document.getElementById(`settings-tab-${next.id}`)?.focus(),
    );
  };
  return (
    <div class="settings-page">
      <div class="page-heading">
        <span class="eyebrow">PREFERENCES</span>
        <h1>设置</h1>
        <p>{description()}</p>
      </div>
      <div
        class="settings-tabs"
        role="tablist"
        aria-label="设置分类"
        onKeyDown={(event) => {
          if (event.key === "ArrowRight") {
            event.preventDefault();
            moveTab(1);
          } else if (event.key === "ArrowLeft") {
            event.preventDefault();
            moveTab(-1);
          }
        }}
      >
        <For each={tabs()}>
          {(tab) => (
            <button
              type="button"
              classList={{
                "settings-tab": true,
                active: props.tab === tab.id,
              }}
              role="tab"
              id={`settings-tab-${tab.id}`}
              aria-selected={props.tab === tab.id}
              aria-controls={`settings-panel-${tab.id}`}
              tabindex={props.tab === tab.id ? 0 : -1}
              onClick={() => props.onTab(tab.id)}
            >
              <Dynamic component={tab.icon} size={14} />
              {tab.label}
            </button>
          )}
        </For>
      </div>
      <Show when={props.state.config}>
        <div class="settings-summary">
          <span title={props.state.endpoint.baseUrl}>
            {props.state.endpoint.name}
          </span>
          <span title={props.state.config?.groupName}>
            {props.state.config?.groupName}
          </span>
          <span title={props.state.config?.model}>
            {props.state.config?.model}
          </span>
          <span title={props.state.config?.workspace}>
            {folderName(props.state.config?.workspace) || "默认工作区"}
          </span>
        </div>
      </Show>
      <Show when={props.tab === "endpoint"}>
        <EndpointTab
          state={props.state}
          streaming={props.streaming}
          onEndpoint={props.onEndpoint}
        />
      </Show>
      <Show when={props.tab === "model"}>
        <ModelTab
          state={props.state}
          groups={props.groups}
          busy={props.busy}
          streaming={props.streaming}
          onConfigured={props.onConfigured}
          onRefreshGroups={props.onRefreshGroups}
        />
      </Show>
      <Show when={props.tab === "plugins"}>
        <PluginsTab
          streaming={props.streaming}
          onChange={props.onPluginsChange}
        />
      </Show>
      <For each={props.pages}>
        {(page) => (
          <Show when={props.tab === pluginSettingsTab(page.id)}>
            <section
              class="settings-card settings-mixin-page"
              role="tabpanel"
              id={`settings-panel-${pluginSettingsTab(page.id)}`}
              aria-labelledby={`settings-tab-${pluginSettingsTab(page.id)}`}
            >
              <iframe
                title={page.name}
                srcdoc={withBridge(page.html)}
                sandbox="allow-scripts"
              />
            </section>
          </Show>
        )}
      </For>
    </div>
  );
}

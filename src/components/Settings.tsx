import { createEffect, createSignal, For, Show } from "solid-js";
import {
  Check,
  ChevronDown,
  Folder,
  Globe2,
  KeyRound,
  LoaderCircle,
  Save,
  RefreshCw,
} from "lucide-solid";
import type { AppState, ConfigSummary, Endpoint, Group } from "../lib/api";
import { command, defaultModel, desktop, errorMessage, ENDPOINTS } from "../lib/api";

interface Props {
  state: AppState;
  groups: Group[];
  busy: boolean;
  streaming: boolean;
  onEndpoint: (index: number) => Promise<void>;
  onConfigured: (config: ConfigSummary) => void;
  onRefreshGroups: () => Promise<void>;
}

export function EndpointSelect(props: {
  endpoint: Endpoint;
  endpoints: Endpoint[];
  disabled?: boolean;
  onChange: (index: number) => void;
}) {
  return (
    <div class="select-wrap endpoint-select">
      <Globe2 size={15} aria-hidden="true" />
      <select
        aria-label="API 线路"
        value={props.endpoint.index}
        disabled={props.disabled}
        onChange={(event) => props.onChange(Number(event.currentTarget.value))}
      >
        <For each={props.endpoints}>
          {(endpoint) => (
            <option value={endpoint.index} selected={endpoint.index === props.endpoint.index}>
              {ENDPOINTS[endpoint.index]?.name ?? endpoint.name}
            </option>
          )}
        </For>
      </select>
      <ChevronDown size={14} aria-hidden="true" />
    </div>
  );
}

export default function Settings(props: Props) {
  const [groupId, setGroupId] = createSignal(0);
  const [model, setModel] = createSignal("");
  const [workspace, setWorkspace] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal("");
  const [success, setSuccess] = createSignal(false);
  createEffect(() => {
    const config = props.state.config;
    setGroupId(config?.groupId ?? props.groups[0]?.id ?? 0);
    setModel(config?.model ?? defaultModel(props.groups[0]));
    setWorkspace(config?.workspace ?? "");
  });
  const save = async (event: SubmitEvent) => {
    event.preventDefault();
    setError("");
    setSuccess(false);
    if (!groupId() || !model().trim()) {
      setError("请选择用户分组并填写模型名称。");
      return;
    }
    setSaving(true);
    try {
      const config = await command<ConfigSummary>("configure", {
        groupId: groupId(),
        model: model().trim(),
        workspace: workspace().trim(),
      });
      props.onConfigured(config);
      setSuccess(true);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };
  return (
    <div class="settings-page">
      <div class="page-heading">
        <span class="eyebrow">PREFERENCES</span>
        <h1>设置</h1>
        <p>账户与工作区</p>
      </div>
      <section class="settings-section">
        <div class="section-heading">
          <Globe2 size={19} />
          <h2>API 线路</h2>
        </div>
        <div class="setting-row">
          <label for="endpoint-settings">当前线路</label>
          <EndpointSelect
            endpoint={props.state.endpoint}
            endpoints={props.state.endpoints}
            disabled={!desktop || props.busy || props.streaming}
            onChange={(index) => void props.onEndpoint(index)}
          />
        </div>
        <div class="setting-row endpoint-address">
          <span>服务地址</span>
          <code>{props.state.endpoint.baseUrl}</code>
        </div>
      </section>
      <form onSubmit={save}>
        <section class="settings-section">
          <div class="section-heading">
            <KeyRound size={19} />
            <h2>模型配置</h2>
            <button
              type="button"
              class="icon-button section-action"
              title="刷新用户分组"
              aria-label="刷新用户分组"
              disabled={!props.state.authenticated || props.busy || saving()}
              onClick={() => void props.onRefreshGroups()}
            >
              <RefreshCw size={16} class={props.busy ? "spin" : ""} />
            </button>
          </div>
          <div class="setting-row">
            <label for="group">用户分组</label>
            <div class="select-wrap">
              <select
                id="group"
                value={groupId()}
                disabled={
                  !props.state.authenticated || saving() || props.streaming
                }
                onChange={(event) => {
                  const id = Number(event.currentTarget.value);
                  setGroupId(id);
                  setModel(
                    defaultModel(props.groups.find((group) => group.id === id)),
                  );
                  setSuccess(false);
                }}
              >
                <Show
                  when={props.groups.length}
                  fallback={<option value={0}>暂无可用分组</option>}
                >
                  <For each={props.groups}>
                    {(group) => <option value={group.id}>{group.name}</option>}
                  </For>
                </Show>
              </select>
              <ChevronDown size={14} />
            </div>
          </div>
          <div class="setting-row">
            <label for="model">模型</label>
            <input
              id="model"
              value={model()}
              onInput={(event) => {
                setModel(event.currentTarget.value);
                setSuccess(false);
              }}
              required
              disabled={
                !props.state.authenticated || saving() || props.streaming
              }
              placeholder="claude-sonnet-4-6"
              spellcheck={false}
            />
          </div>
        </section>
        <section class="settings-section">
          <div class="section-heading">
            <Folder size={19} />
            <h2>工作区</h2>
          </div>
          <div class="setting-row">
            <label for="workspace">目录</label>
            <input
              id="workspace"
              value={workspace()}
              onInput={(event) => {
                setWorkspace(event.currentTarget.value);
                setSuccess(false);
              }}
              disabled={
                !props.state.authenticated || saving() || props.streaming
              }
              placeholder="默认工作区"
              spellcheck={false}
            />
          </div>
          <Show when={props.state.config}>
            <div class="setting-row config-path">
              <span>配置文件</span>
              <code>{props.state.config?.configPath}</code>
            </div>
          </Show>
        </section>
        <Show when={error()}>
          <div role="alert" class="notice error-notice">
            {error()}
          </div>
        </Show>
        <div class="settings-footer">
          <span class={success() ? "save-result success" : "save-result"}>
            <Show when={success()}>
              <Check size={15} />
              配置已写入
            </Show>
          </span>
          <button
            class="primary-button"
            type="submit"
            disabled={
              !props.state.authenticated ||
              props.streaming ||
              props.busy ||
              saving()
            }
          >
            {saving() ? (
              <LoaderCircle size={16} class="spin" />
            ) : (
              <Save size={16} />
            )}
            {saving() ? "正在配置" : "一键配置"}
          </button>
        </div>
      </form>
    </div>
  );
}

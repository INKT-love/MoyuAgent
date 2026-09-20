import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { ChevronDown, KeyRound, RefreshCw } from "lucide-solid";
import type { AppState, ConfigSummary, Group, GroupModel } from "../../lib/api";
import { command, errorMessage, pickModel } from "../../lib/api";
import { SaveBar } from "./SaveBar";

export function ModelTab(props: {
  state: AppState;
  groups: Group[];
  busy: boolean;
  streaming: boolean;
  onConfigured: (config: ConfigSummary) => void;
  onRefreshGroups: () => Promise<void>;
}) {
  const [groupId, setGroupId] = createSignal(0);
  const [model, setModel] = createSignal("");
  const [models, setModels] = createSignal<GroupModel[]>([]);
  const [modelQuery, setModelQuery] = createSignal("");
  const [modelsLoading, setModelsLoading] = createSignal(false);
  const [modelsTick, setModelsTick] = createSignal(0);
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal("");
  const [success, setSuccess] = createSignal(false);
  let groupSelect: HTMLSelectElement | undefined;
  let modelSelect: HTMLSelectElement | undefined;
  let forceModelsRefresh = false;
  const authenticated = createMemo(() => props.state.authenticated);
  const savedGroupId = createMemo(() => props.state.config?.groupId ?? 0);
  const savedModel = createMemo(() => props.state.config?.model ?? "");

  createEffect(() => {
    setGroupId(savedGroupId() || props.groups[0]?.id || 0);
    setModel(savedModel());
  });

  createEffect(() => {
    const id = groupId();
    modelsTick();
    if (!id || !authenticated()) {
      setModels([]);
      setModelsLoading(false);
      return;
    }
    let cancelled = false;
    const refresh = forceModelsRefresh;
    forceModelsRefresh = false;
    setModelsLoading(true);
    void command<GroupModel[]>("get_group_models", {
      groupId: id,
      ...(refresh ? { refresh: true } : {}),
    })
      .then((list) => {
        if (cancelled) return;
        setModels(list);
        setModel((current) =>
          pickModel(
            list,
            current || (savedGroupId() === id ? savedModel() : undefined),
          ),
        );
      })
      .catch((err) => {
        if (cancelled) return;
        setModels([]);
        setError(errorMessage(err));
      })
      .finally(() => {
        if (!cancelled) setModelsLoading(false);
      });
    onCleanup(() => {
      cancelled = true;
    });
  });

  const visibleModels = createMemo(() => {
    const selected = model();
    const query = modelQuery().trim().toLowerCase();
    const list = models();
    if (!query) return list;
    return list.filter(
      (item) =>
        item.id === selected ||
        item.id.toLowerCase().includes(query) ||
        item.name.toLowerCase().includes(query),
    );
  });

  // Reapply the selection after options are rebuilt, even when the saved value
  // is unchanged. Otherwise native selects can fall back to the first option.
  createEffect(() => {
    props.groups;
    const selected = groupId();
    if (groupSelect) groupSelect.value = String(selected);
  });
  createEffect(() => {
    visibleModels();
    const selected = model();
    if (modelSelect) modelSelect.value = selected;
  });

  const dirty = createMemo(() => {
    const config = props.state.config;
    if (!config) return Boolean(groupId() && model());
    return config.groupId !== groupId() || config.model !== model();
  });
  const locked = () => !authenticated() || saving() || props.streaming;

  const save = async (event: SubmitEvent) => {
    event.preventDefault();
    setError("");
    setSuccess(false);
    if (!groupId() || !model().trim()) {
      setError("请选择用户分组和模型。");
      return;
    }
    setSaving(true);
    try {
      const config = await command<ConfigSummary>("configure", {
        groupId: groupId(),
        model: model().trim(),
        workspace: props.state.config?.workspace ?? "",
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
    <form
      class="settings-form"
      onSubmit={save}
      role="tabpanel"
      id="settings-panel-model"
      aria-labelledby="settings-tab-model"
    >
      <section class="settings-card">
        <div class="section-heading">
          <KeyRound size={18} />
          <h2>模型配置</h2>
        </div>
        <p class="settings-copy">
          分组和模型会在首次加载后保存在本地，可点刷新从 Sub2API 更新。只写入当前选择，工作区保持不变。
        </p>
        <div class="setting-grid">
          <div class="setting-field">
            <div class="setting-label-row">
              <label for="group">用户分组</label>
              <button
                type="button"
                class="field-refresh"
                title="刷新用户分组"
                aria-label="刷新用户分组"
                disabled={!authenticated() || props.busy || saving()}
                onClick={() => {
                  void props.onRefreshGroups();
                }}
              >
                <RefreshCw size={13} class={props.busy ? "spin" : ""} />
              </button>
            </div>
            <div class="select-wrap">
              <select
                ref={groupSelect}
                id="group"
                value={groupId()}
                disabled={locked()}
                onChange={(event) => {
                  setGroupId(Number(event.currentTarget.value));
                  setModel("");
                  setModels([]);
                  setModelQuery("");
                  setSuccess(false);
                }}
              >
                <Show
                  when={props.groups.length}
                  fallback={<option value={0}>暂无可用分组</option>}
                >
                  <For each={props.groups}>
                    {(group) => (
                      <option value={group.id}>{group.name}</option>
                    )}
                  </For>
                </Show>
              </select>
              <ChevronDown size={14} />
            </div>
          </div>
          <div class="setting-field">
            <div class="setting-label-row">
              <label for="model">模型</label>
              <button
                type="button"
                class="field-refresh"
                title="刷新模型列表"
                aria-label="刷新模型列表"
                disabled={
                  locked() || modelsLoading() || !groupId() || props.busy
                }
                onClick={() => {
                  forceModelsRefresh = true;
                  setError("");
                  setModelsTick((tick) => tick + 1);
                }}
              >
                <RefreshCw
                  size={13}
                  class={modelsLoading() ? "spin" : ""}
                />
              </button>
            </div>
            <Show when={models().length > 8}>
              <input
                class="model-filter"
                type="search"
                value={modelQuery()}
                placeholder="筛选模型名称或 ID"
                disabled={locked() || modelsLoading()}
                onInput={(event) => setModelQuery(event.currentTarget.value)}
              />
            </Show>
            <div class="select-wrap">
              <select
                ref={modelSelect}
                id="model"
                value={model()}
                disabled={locked() || modelsLoading()}
                onChange={(event) => {
                  setModel(event.currentTarget.value);
                  setSuccess(false);
                }}
              >
                <Show
                  when={visibleModels().length}
                  fallback={
                    <option value="">
                      {modelsLoading() ? "正在加载模型" : "暂无可用模型"}
                    </option>
                  }
                >
                  <For each={visibleModels()}>
                    {(item) => (
                      <option value={item.id} title={item.id}>
                        {item.name === item.id
                          ? item.id
                          : `${item.name} · ${item.id}`}
                      </option>
                    )}
                  </For>
                </Show>
              </select>
              <ChevronDown size={14} />
            </div>
          </div>
        </div>
      </section>
      <Show when={error()}>
        <div role="alert" class="notice error-notice">
          {error()}
        </div>
      </Show>
      <SaveBar
        saving={saving()}
        success={success()}
        dirty={dirty()}
        disabled={
          locked() || props.busy || modelsLoading() || !model()
        }
      />
    </form>
  );
}

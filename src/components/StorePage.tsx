import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import { RefreshCw, Search, Store, Trash2 } from "lucide-solid";
import {
  command,
  desktop,
  errorMessage,
  type SkillInfo,
  type StoreItem,
  type StoreKind,
  type StorePreview,
  type StoreView,
} from "../lib/api";
import { matchesQuery } from "../lib/store";
import { ErrorNotice } from "./ErrorNotice";

interface Props {
  streaming: boolean;
  /** Called after anything that changes the installed plugins or skills. */
  onChange?: () => void;
}

const RISK_LABEL = { high: "高", medium: "中", low: "低" } as const;

export function StorePage(props: Props) {
  const [kind, setKind] = createSignal<StoreKind>("plugin");
  const [query, setQuery] = createSignal("");
  const [view, setView] = createSignal<StoreView | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [busyId, setBusyId] = createSignal("");
  const [error, setError] = createSignal("");
  const [preview, setPreview] = createSignal<StorePreview | null>(null);
  const [urlDraft, setUrlDraft] = createSignal("");
  let dialog: HTMLDialogElement | undefined;

  const locked = () => !desktop || props.streaming || loading() || !!busyId();

  const load = async () => {
    setLoading(true);
    setError("");
    try {
      const next = await command<StoreView>("store_catalog");
      setView(next);
      setUrlDraft(next.url);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  onMount(() => {
    if (desktop) void load();
  });

  const items = createMemo(() =>
    (view()?.items ?? []).filter(
      (item) => item.kind === kind() && matchesQuery(item, query()),
    ),
  );

  // Skills installed on disk that the current catalog does not list (or the catalog is offline).
  const orphanSkills = createMemo<SkillInfo[]>(() => {
    const listed = new Set((view()?.items ?? []).map((item) => item.id));
    return (view()?.skills ?? []).filter((skill) => !listed.has(skill.name));
  });

  const closeDialog = () => dialog?.close();

  const cancelPreview = async () => {
    const current = preview();
    setPreview(null);
    closeDialog();
    if (current) await command("store_cancel", { token: current.token }).catch(() => undefined);
  };

  const prepare = async (item: StoreItem) => {
    if (locked()) return;
    setBusyId(item.id);
    setError("");
    try {
      const next = await command<StorePreview>("store_prepare", {
        kind: item.kind,
        id: item.id,
      });
      setPreview(next);
      dialog?.showModal();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusyId("");
    }
  };

  const confirmInstall = async () => {
    const current = preview();
    if (!current || busyId()) return;
    setBusyId(current.id);
    setError("");
    try {
      setView(await command<StoreView>("store_install", { token: current.token }));
      props.onChange?.();
      setPreview(null);
      closeDialog();
    } catch (err) {
      setError(errorMessage(err));
      setPreview(null);
      closeDialog();
    } finally {
      setBusyId("");
    }
  };

  const mutateSkill = async (name: string, work: () => Promise<SkillInfo[]>) => {
    if (locked()) return;
    setBusyId(name);
    setError("");
    try {
      const skills = await work();
      setView((current) => (current ? { ...current, skills } : current));
      await load();
      props.onChange?.();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusyId("");
    }
  };

  const setPluginEnabled = async (item: StoreItem, enabled: boolean) => {
    if (locked()) return;
    setBusyId(item.id);
    setError("");
    try {
      await command("set_plugin_enabled", { id: item.id, enabled });
      await load();
      props.onChange?.();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusyId("");
    }
  };

  const uninstall = async (item: StoreItem) => {
    if (item.kind === "skill") {
      await mutateSkill(item.id, () => command<SkillInfo[]>("uninstall_skill", { name: item.id }));
      return;
    }
    if (locked()) return;
    setBusyId(item.id);
    setError("");
    try {
      await command("uninstall_plugin", { id: item.id });
      await load();
      props.onChange?.();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusyId("");
    }
  };

  const toggle = (item: StoreItem, enabled: boolean) =>
    item.kind === "skill"
      ? mutateSkill(item.id, () => command<SkillInfo[]>("set_skill_enabled", { name: item.id, enabled }))
      : setPluginEnabled(item, enabled);

  const saveUrl = async () => {
    if (locked()) return;
    setError("");
    try {
      await command<string>("store_set_url", { url: urlDraft() });
      await load();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  return (
    <div class="store-page">
      <div class="page-heading">
        <span class="eyebrow">STORE</span>
        <h1>商店</h1>
        <p>浏览并安装插件与技能，安装前会列出它能做什么</p>
      </div>

      <section class="settings-card" aria-label="商店">
        <div class="section-heading">
          <Store size={18} />
          <h2>目录</h2>
          <button
            type="button"
            class="text-button section-action"
            disabled={locked()}
            onClick={() => void load()}
          >
            <RefreshCw size={14} />
            刷新
          </button>
        </div>

        <div class="store-toolbar">
          <div class="store-tabs" role="tablist" aria-label="商店分类">
            <For each={[["plugin", "插件"], ["skill", "技能"]] as const}>
              {([id, label]) => (
                <button
                  type="button"
                  role="tab"
                  aria-selected={kind() === id}
                  classList={{ active: kind() === id }}
                  onClick={() => setKind(id)}
                >
                  {label}
                </button>
              )}
            </For>
          </div>
          <label class="store-search">
            <Search size={14} />
            <input
              type="search"
              value={query()}
              placeholder="按名称、说明或标签搜索"
              aria-label="搜索商店"
              onInput={(event) => setQuery(event.currentTarget.value)}
            />
          </label>
        </div>

        <Show when={error()}>
          <ErrorNotice error={error()} class="notice error-notice" />
        </Show>
        <Show when={!desktop}>
          <p class="settings-copy">商店只在桌面版里可用。</p>
        </Show>

        <Show when={loading() && !view()}>
          <p class="settings-copy" role="status">正在读取商店目录…</p>
        </Show>

        <Show when={view()}>
          <ul class="plugin-list store-list">
            <For
              each={items()}
              fallback={
                <li class="settings-copy">
                  {query() ? "没有符合搜索的条目。" : "这个分类里还没有内容。"}
                </li>
              }
            >
              {(item) => (
                <li class="plugin-row store-row">
                  <div class="store-info">
                    <strong>{item.name}</strong>
                    <small>
                      v{item.version}
                      {item.author ? ` · ${item.author}` : ""}
                      {item.installed ? " · 已安装" : ""}
                      {item.updateAvailable
                        ? ` · 可更新（当前 v${item.installedVersion}）`
                        : ""}
                    </small>
                    <em>{item.description}</em>
                    <Show when={item.tags.length}>
                      <span class="store-tags">
                        <For each={item.tags}>{(tag) => <span>{tag}</span>}</For>
                      </span>
                    </Show>
                  </div>
                  <div class="store-actions">
                    <Show when={item.installed}>
                      <label class="store-switch">
                        <input
                          type="checkbox"
                          checked={item.enabled}
                          disabled={locked()}
                          onChange={(event) => void toggle(item, event.currentTarget.checked)}
                        />
                        启用
                      </label>
                    </Show>
                    <button
                      type="button"
                      class="text-button store-install"
                      disabled={locked() || (item.installed && !item.updateAvailable)}
                      onClick={() => void prepare(item)}
                    >
                      {busyId() === item.id
                        ? "处理中…"
                        : item.updateAvailable
                          ? "更新"
                          : item.installed
                            ? "已是最新"
                            : "安装"}
                    </button>
                    <Show when={item.installed}>
                      <button
                        type="button"
                        class="icon-button"
                        title="卸载"
                        aria-label={`卸载 ${item.name}`}
                        disabled={locked()}
                        onClick={() => void uninstall(item)}
                      >
                        <Trash2 size={14} />
                      </button>
                    </Show>
                  </div>
                </li>
              )}
            </For>
          </ul>
        </Show>

        <Show when={kind() === "skill" && orphanSkills().length}>
          <h3 class="store-subtitle">已安装但不在当前目录里的技能</h3>
          <ul class="plugin-list store-list">
            <For each={orphanSkills()}>
              {(skill) => (
                <li class="plugin-row store-row">
                  <div class="store-info">
                    <strong>{skill.name}</strong>
                    <em>{skill.description || "无说明"}</em>
                  </div>
                  <div class="store-actions">
                    <label class="store-switch">
                      <input
                        type="checkbox"
                        checked={skill.enabled}
                        disabled={locked()}
                        onChange={(event) =>
                          void mutateSkill(skill.name, () =>
                            command<SkillInfo[]>("set_skill_enabled", {
                              name: skill.name,
                              enabled: event.currentTarget.checked,
                            }),
                          )
                        }
                      />
                      启用
                    </label>
                    <button
                      type="button"
                      class="icon-button"
                      title="卸载"
                      aria-label={`卸载 ${skill.name}`}
                      disabled={locked()}
                      onClick={() =>
                        void mutateSkill(skill.name, () =>
                          command<SkillInfo[]>("uninstall_skill", { name: skill.name }),
                        )
                      }
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </li>
              )}
            </For>
          </ul>
        </Show>

        <div class="setting-field store-source">
          <div class="setting-label-row">
            <label for="store-url">商店地址</label>
          </div>
          <div class="store-source-row">
            <input
              id="store-url"
              type="url"
              value={urlDraft()}
              spellcheck={false}
              onInput={(event) => setUrlDraft(event.currentTarget.value)}
            />
            <button
              type="button"
              class="text-button"
              disabled={locked() || urlDraft() === (view()?.url ?? "")}
              onClick={() => void saveUrl()}
            >
              保存
            </button>
          </div>
          <p class="settings-copy">只接受 https 地址。留空使用默认商店。</p>
        </div>
      </section>

      <dialog
        ref={dialog}
        class="agreement-dialog store-dialog"
        aria-labelledby="store-dialog-title"
        onCancel={(event) => {
          event.preventDefault();
          void cancelPreview();
        }}
      >
        <Show when={preview()}>
          {(current) => (
            <>
              <div class="dialog-heading">
                <h2 id="store-dialog-title">
                  {current().replaces ? "更新" : "安装"} {current().name}
                </h2>
              </div>
              <p class="settings-copy">
                v{current().version}。下面是从安装包里实际读出的内容，不是商店自己的描述。
                {current().replaces ? "会替换已安装的版本，它的设置会保留。" : ""}
              </p>
              <ul class="store-capabilities">
                <For each={current().capabilities}>
                  {(item) => (
                    <li classList={{ [`risk-${item.risk}`]: true }}>
                      <span class="risk-badge">风险 {RISK_LABEL[item.risk]}</span>
                      <strong>{item.title}</strong>
                      <p>{item.detail}</p>
                    </li>
                  )}
                </For>
              </ul>
              <div class="store-dialog-actions">
                <button type="button" class="text-button" onClick={() => void cancelPreview()}>
                  取消
                </button>
                <button
                  type="button"
                  class="text-button store-install"
                  disabled={!!busyId()}
                  onClick={() => void confirmInstall()}
                >
                  {busyId() ? "安装中…" : "确认安装"}
                </button>
              </div>
            </>
          )}
        </Show>
      </dialog>
    </div>
  );
}

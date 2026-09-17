import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { Channel } from "@tauri-apps/api/core";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  ChevronRight,
  Code2,
  Copy,
  Eye,
  EyeOff,
  Folder,
  FolderOpen,
  LogOut,
  MessageSquare,
  PanelLeftOpen,
  Plus,
  RotateCw,
  Settings2,
  ShieldCheck,
  Square,
  Terminal,
  X,
  LoaderCircle,
  AlertCircle,
} from "lucide-solid";
import Settings, {
  EndpointSelect,
  pluginSettingsTab,
  type SettingsTab,
} from "./components/Settings";
import {
  command,
  defaultModel,
  desktop,
  errorMessage,
  folderName,
  initialState,
  pickModel,
  supportedGroups,
  type AppState,
  type ConfigSummary,
  type Endpoint,
  type Group,
  type GroupModel,
  type LoginResult,
  type PublicSettings,
} from "./lib/api";
import {
  mixins,
  type PluginMixin,
  type UiInjection,
} from "./lib/mixin";
import {
  createStreamController,
  type StreamEvent,
} from "./lib/stream-controller";

interface Message {
  id: string;
  role: "user" | "assistant";
  text: string;
  state?: "streaming" | "completed" | "failed" | "cancelled";
  error?: string;
}
interface Conversation {
  id: string;
  title: string;
  messages: Message[];
  sessionId?: string;
}
const newConversation = (): Conversation => ({
  id: crypto.randomUUID(),
  title: "新任务",
  messages: [],
});

export default function App() {
  const [state, setState] = createSignal<AppState>(initialState);
  const [initializing, setInitializing] = createSignal(desktop);
  const [view, setView] = createSignal<"chat" | "settings">("chat");
  const [settingsTab, setSettingsTab] = createSignal<SettingsTab>("model");
  const [preview, setPreview] = createSignal(false);
  const [sidebarOpen, setSidebarOpen] = createSignal(false);
  const [email, setEmail] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [totpCode, setTotpCode] = createSignal("");
  const [twoFactorToken, setTwoFactorToken] = createSignal<string | null>(null);
  const [passwordVisible, setPasswordVisible] = createSignal(false);
  const [agreement, setAgreement] = createSignal(false);
  const [publicSettings, setPublicSettings] = createSignal<PublicSettings>({
    loginAgreementRequired: false,
    loginAgreementUrl: null,
    registrationUrl: null,
  });
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [groups, setGroups] = createSignal<Group[]>([]);
  const firstConversation = newConversation();
  const [conversations, setConversations] = createSignal<Conversation[]>([
    firstConversation,
  ]);
  const [activeConversationId, setActiveConversationId] = createSignal(
    firstConversation.id,
  );
  const [prompt, setPrompt] = createSignal("");
  const [mainMixins, setMainMixins] = createSignal<UiInjection[]>([]);
  const [settingsMixins, setSettingsMixins] = createSignal<UiInjection[]>([]);
  const [hiddenPluginUi, setHiddenPluginUi] = createSignal<string[]>([]);
  const [streaming, setStreaming] = createSignal(false);
  const [streamStatus, setStreamStatus] = createSignal("");
  const [copiedId, setCopiedId] = createSignal("");
  const [nearBottom, setNearBottom] = createSignal(true);
  const conversation = createMemo(
    () =>
      conversations().find((item) => item.id === activeConversationId()) ??
      conversations()[0],
  );
  let transcript: HTMLDivElement | undefined;
  let composer: HTMLTextAreaElement | undefined;
  let activeRequestId: string | undefined;
  let failActiveStream: ((message: string) => void) | undefined;
  let unlisten: (() => void) | undefined;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let scrollFrame: number | undefined;
  let disposed = false;
  let agreementDialog: HTMLDialogElement | undefined;
  let mixinObserver: MutationObserver | undefined;
  let mixinFrame: number | undefined;

  const patchConversation = (
    id: string,
    patch: (item: Conversation) => Conversation,
  ) =>
    setConversations((items) =>
      items.map((item) => (item.id === id ? patch(item) : item)),
    );
  const refreshState = async () => {
    const next = await command<AppState>("get_app_state");
    if (!disposed) setState(next);
  };
  const refreshGroups = async () => {
    setBusy(true);
    try {
      setGroups(supportedGroups(await command<Group[]>("get_groups")));
      await refreshState();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const refreshMixins = async () => {
    if (!desktop) return;
    mixins.reset();
    try {
      const list = await command<PluginMixin[]>("list_plugin_mixins");
      for (const mixin of list) mixins.register(mixin);
      const pages = mixins.pages();
      setMainMixins(mixins.overlays());
      setSettingsMixins(pages);
      setSettingsTab((tab) =>
        tab === "endpoint" ||
        tab === "model" ||
        tab === "plugins" ||
        pages.some((page) => pluginSettingsTab(page.id) === tab)
          ? tab
          : "plugins",
      );
      mixins.applyDom();
    } catch {
      setMainMixins([]);
      setSettingsMixins([]);
    }
  };
  onMount(async () => {
    if (!desktop) return;
    try {
      await refreshState();
      setPublicSettings(await command<PublicSettings>("get_public_settings"));
      await refreshState();
      if (state().authenticated)
        setGroups(supportedGroups(await command<Group[]>("get_groups")));
      await refreshMixins();
      mixinObserver = new MutationObserver(() => {
        if (mixinFrame !== undefined) return;
        mixinFrame = requestAnimationFrame(() => {
          mixinFrame = undefined;
          mixins.applyDom();
        });
      });
      mixinObserver.observe(document.body, { childList: true, subtree: true });
    } catch (err) {
      if (!disposed) setError(errorMessage(err));
    } finally {
      if (!disposed) setInitializing(false);
    }
  });
  onCleanup(() => {
    disposed = true;
    mixinObserver?.disconnect();
    if (mixinFrame !== undefined) cancelAnimationFrame(mixinFrame);
    mixins.reset();
    setPassword("");
    setTotpCode("");
    setTwoFactorToken(null);
    unlisten?.();
    if (copyTimer !== undefined) clearTimeout(copyTimer);
    if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame);
  });

  let switchingEndpoint = false;
  const switchEndpoint = async (index: number) => {
    if (streaming() || switchingEndpoint || state().endpoint.index === index)
      return;
    switchingEndpoint = true;
    const selected = state().endpoints.find((item) => item.index === index);
    if (selected) setState((current) => ({ ...current, endpoint: selected }));
    setError("");
    try {
      const endpoint = await command<Endpoint>("switch_endpoint", { index });
      setState((current) => ({ ...current, endpoint }));
      await refreshState();
    } catch (err) {
      setError(errorMessage(err));
      await refreshState().catch(() => undefined);
    } finally {
      switchingEndpoint = false;
    }
  };
  const openSettings = (tab: SettingsTab) => {
    setSettingsTab(tab);
    setView("settings");
    setSidebarOpen(false);
  };
  const [workspaceMenu, setWorkspaceMenu] = createSignal(false);
  const chooseWorkspace = async () => {
    if (!desktop || streaming() || busy() || !state().authenticated) return;
    setWorkspaceMenu(false);
    setError("");
    try {
      const config = await command<ConfigSummary | null>("choose_workspace");
      if (config) setState((current) => ({ ...current, config }));
      await refreshState();
    } catch (err) {
      setError(errorMessage(err));
    }
  };
  const applyWorkspace = async (path: string) => {
    if (streaming() || busy() || !state().authenticated) return;
    setWorkspaceMenu(false);
    setError("");
    try {
      const config = await command<ConfigSummary>("configure", {
        groupId: state().config?.groupId,
        model: state().config?.model,
        workspace: path,
      });
      setState((current) => ({ ...current, config }));
      await refreshState();
    } catch (err) {
      setError(errorMessage(err));
    }
  };
  const revealWorkspace = async () => {
    if (!desktop || !state().config) return;
    setWorkspaceMenu(false);
    try {
      await command<void>("reveal_workspace");
    } catch (err) {
      setError(errorMessage(err));
    }
  };
  const login = async (event: SubmitEvent) => {
    event.preventDefault();
    if (busy() || !desktop) return;
    setBusy(true);
    setError("");
    try {
      const result = twoFactorToken()
        ? await command<LoginResult>("complete_two_factor", {
            tempToken: twoFactorToken(),
            totpCode: totpCode().trim(),
          })
        : await command<LoginResult>("login", {
            email: email().trim(),
            password: password(),
            acceptedAgreement: agreement(),
          });
      setPassword("");
      if (result.requiresTwoFactor) {
        if (!result.tempToken) throw new Error("登录响应缺少两步验证信息。");
        setTwoFactorToken(result.tempToken);
        return;
      }
      setTotpCode("");
      setTwoFactorToken(null);
      await refreshState();
      const loggedIn = state();
      const availableGroups = supportedGroups(
        await command<Group[]>("get_groups"),
      );
      setGroups(availableGroups);
      if (!loggedIn.config && availableGroups.length) {
        setStreamStatus("正在配置工作区");
        const group = availableGroups[0];
        let model = defaultModel(group);
        try {
          model = pickModel(
            await command<GroupModel[]>("get_group_models", {
              groupId: group.id,
            }),
            model,
          );
        } catch {
          // Keep the platform default if the group catalog is unavailable.
        }
        const config = await command<ConfigSummary>("configure", {
          groupId: group.id,
          model,
          workspace: "",
        });
        setState((current) => ({ ...current, config }));
      } else if (!loggedIn.config) {
        setError("当前账户暂无可用分组，请在账户后台检查分组权限。");
        setView("settings");
      }
      await refreshState();
    } catch (err) {
      setError(errorMessage(err));
      if (state().authenticated && !state().config) setView("settings");
    } finally {
      setBusy(false);
      setStreamStatus("");
      void refreshState().catch(() => undefined);
    }
  };
  const logout = async () => {
    if (streaming()) return;
    setBusy(true);
    setError("");
    try {
      await command<void>("logout");
      setState((current) => ({
        ...current,
        authenticated: false,
        user: null,
        config: null,
      }));
      setGroups([]);
      setView("chat");
      const fresh = newConversation();
      setConversations([fresh]);
      setActiveConversationId(fresh.id);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const startNew = () => {
    if (streaming()) return;
    if (conversation().messages.length) {
      const fresh = newConversation();
      setConversations((items) => [fresh, ...items]);
      setActiveConversationId(fresh.id);
    }
    setView("chat");
    setPrompt("");
    setSidebarOpen(false);
    setError("");
    composer?.focus();
  };
  const stop = async () => {
    if (!activeRequestId) return;
    setStreamStatus("正在停止");
    try {
      await command<void>("cancel_stream", { requestId: activeRequestId });
    } catch (err) {
      const message = errorMessage(err);
      setError(message);
      failActiveStream?.(message);
    }
  };
  const sendMessage = async (raw?: unknown) => {
    const text = String(raw ?? "").trim();
    if (
      !text ||
      streaming() ||
      busy() ||
      !state().authenticated ||
      !state().config
    )
      return;
    unlisten?.();
    const requestId = crypto.randomUUID();
    const conversationId = conversation().id;
    const responseId = crypto.randomUUID();
    const sessionId = conversation().sessionId;
    setError("");
    setPrompt("");
    setStreaming(true);
    setStreamStatus("正在连接");
    setNearBottom(true);
    activeRequestId = requestId;
    patchConversation(conversationId, (item) => ({
      ...item,
      title: item.messages.length ? item.title : text.slice(0, 42),
      messages: [
        ...item.messages,
        { id: crypto.randomUUID(), role: "user", text },
        { id: responseId, role: "assistant", text: "", state: "streaming" },
      ],
    }));
    const patchResponse = (patch: (message: Message) => Message) =>
      patchConversation(conversationId, (item) => ({
        ...item,
        messages: item.messages.map((message) =>
          message.id === responseId ? patch(message) : message,
        ),
      }));
    const channel = new Channel<StreamEvent>();
    const controller = createStreamController({
      requestId,
      append: (chunk) => {
        patchResponse((message) => ({
          ...message,
          text: message.text + chunk,
        }));
        setStreamStatus("正在生成");
      },
      status: (message) => setStreamStatus(message),
      session: (id) =>
        patchConversation(conversationId, (item) => ({
          ...item,
          sessionId: id,
        })),
      terminal: (terminal) => {
        patchResponse((message) => ({
          ...message,
          state: terminal.kind,
          error: terminal.kind === "failed" ? terminal.message : undefined,
        }));
        setStreaming(false);
        setStreamStatus("");
        activeRequestId = undefined;
        failActiveStream = undefined;
        channel.onmessage = () => undefined;
        void refreshState().catch(() => undefined);
      },
      acknowledge: (sequence) =>
        command<void>("ack_stream", { requestId, sequence }),
      cancel: () => command<void>("cancel_stream", { requestId }),
    });
    channel.onmessage = (event) => controller.receive(event);
    failActiveStream = (message) => controller.fail(message);
    // Channel has no Event-style unlisten; detach its callback and dispose our clocks explicitly.
    unlisten = () => {
      channel.onmessage = () => undefined;
      controller.unlisten();
    };
    try {
      await command<void>("start_stream", {
        requestId,
        prompt: text,
        sessionId,
        onEvent: channel,
      });
    } catch (err) {
      controller.fail(errorMessage(err));
    }
  };
  const send = async (event?: SubmitEvent, override?: string) => {
    event?.preventDefault();
    try {
      await mixins.invoke(
        "chat.send",
        (...args: unknown[]) => sendMessage(args[0]),
        [override ?? prompt()],
      );
    } catch (err) {
      setError(errorMessage(err));
    }
  };
  const retry = (assistant: Message) => {
    const messages = conversation().messages;
    const index = messages.findIndex((item) => item.id === assistant.id);
    const user = [...messages.slice(0, index)]
      .reverse()
      .find((item) => item.role === "user");
    if (user) void send(undefined, user.text);
  };
  const copyMessage = async (message: Message) => {
    try {
      await navigator.clipboard.writeText(message.text);
      setCopiedId(message.id);
      if (copyTimer !== undefined) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => setCopiedId(""), 1600);
    } catch {
      setError("无法访问剪贴板。");
    }
  };
  const scrollToBottom = () => {
    if (transcript) transcript.scrollTop = transcript.scrollHeight;
    setNearBottom(true);
  };
  createEffect(() => {
    conversation().messages;
    if (nearBottom()) {
      if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame);
      scrollFrame = requestAnimationFrame(scrollToBottom);
    }
  });
  const handleComposerInput = (element: HTMLTextAreaElement) => {
    setPrompt(element.value);
    element.style.height = "auto";
    element.style.height = `${Math.min(element.scrollHeight, 160)}px`;
  };

  return (
    <div class="app-shell" onClick={() => setWorkspaceMenu(false)}>
      <Show when={sidebarOpen()}>
        <button
          class="sidebar-backdrop"
          aria-label="关闭导航"
          onClick={() => setSidebarOpen(false)}
        />
      </Show>
      <aside classList={{ sidebar: true, "is-open": sidebarOpen() }}>
        <a
          class="brand"
          href="#"
          onClick={(event) => {
            event.preventDefault();
            setView("chat");
          }}
        >
          <img src="/moyu.png" width="32" height="32" class="brand-mark" alt="" />
          <span>
            Moyu<span class="brand-agent">Agent</span>
          </span>
        </a>
        <button
          class="new-chat-button"
          onClick={startNew}
          disabled={streaming()}
        >
          <Plus size={17} />
          新任务
          <span class="button-end">
            <ChevronRight size={14} />
          </span>
        </button>
        <div class="sidebar-nav">
          <button
            classList={{ "nav-button": true, active: view() === "chat" }}
            onClick={() => {
              setView("chat");
              setSidebarOpen(false);
            }}
          >
            <MessageSquare size={17} />
            对话
          </button>
          <button
            classList={{ "nav-button": true, active: view() === "settings" }}
            onClick={() => {
              setView("settings");
              setSidebarOpen(false);
            }}
          >
            <Settings2 size={17} />
            设置
          </button>
        </div>
        <div class="history-section">
          <div class="nav-section-label">
            最近任务
            <span>
              {conversations()
                .filter((item) => item.messages.length)
                .length.toString()
                .padStart(2, "0")}
            </span>
          </div>
          <Show
            when={conversations().some((item) => item.messages.length)}
            fallback={<div class="history-empty">暂无任务</div>}
          >
            <For each={conversations().filter((item) => item.messages.length)}>
              {(item) => (
                <button
                  classList={{
                    "history-item": true,
                    selected:
                      item.id === activeConversationId() && view() === "chat",
                  }}
                  title={item.title}
                  disabled={streaming() && item.id !== activeConversationId()}
                  onClick={() => {
                    setActiveConversationId(item.id);
                    setView("chat");
                    setSidebarOpen(false);
                    setNearBottom(true);
                  }}
                >
                  <MessageSquare size={14} />
                  <span>{item.title}</span>
                </button>
              )}
            </For>
          </Show>
        </div>
        <div class="sidebar-bottom">
          <button
            type="button"
            class="workspace-status"
            title={state().config?.workspace || "选择工作区"}
            disabled={!state().authenticated || streaming() || busy()}
            onClick={() => void chooseWorkspace()}
          >
            <Folder size={16} />
            <div>
              <span>当前工作区</span>
              <strong>
                {folderName(state().config?.workspace) || "未配置"}
              </strong>
            </div>
            <span classList={{ "status-dot": true, ready: !!state().config }} />
          </button>
          <div class="account-row">
            <span class="avatar">
              {state().user?.email.charAt(0).toUpperCase() || "M"}
            </span>
            <div>
              <strong>{state().user?.email || "尚未登录"}</strong>
              <span>
                {state().authenticated ? "Sub2API 账户" : "Moyu Agent"}
              </span>
            </div>
            <Show when={state().authenticated}>
              <button
                class="icon-button"
                title="退出登录"
                aria-label="退出登录"
                disabled={busy() || streaming()}
                onClick={() => void logout()}
              >
                <LogOut size={16} />
              </button>
            </Show>
          </div>
        </div>
      </aside>
      <main class="main-pane">
        <header class="topbar">
          <div class="breadcrumb">
            <button
              class="icon-button mobile-nav"
              aria-label="打开导航"
              title="打开导航"
              onClick={() => setSidebarOpen(true)}
            >
              <PanelLeftOpen size={19} />
            </button>
            <div class="topbar-title">
              <span class="eyebrow">
                {view() === "settings" ? "PREFERENCES" : "WORKSPACE"}
              </span>
              <strong>
                {view() === "settings"
                  ? "设置"
                  : conversation().messages.length
                    ? conversation().title
                    : "新任务"}
              </strong>
            </div>
          </div>
          <div class="topbar-actions">
            <Show when={state().authenticated && view() === "chat"}>
              <button
                class="topbar-chip"
                title="打开设置"
                onClick={() => openSettings("model")}
              >
                <span classList={{ "status-dot": true, ready: !!state().config }} />
                {state().config?.model || "尚未配置模型"}
              </button>
            </Show>
            <span
              classList={{
                "connection-state": true,
                connected: state().authenticated,
              }}
            >
              <span class="status-dot" />
              {state().authenticated
                ? "已连接"
                : desktop
                  ? "未连接"
                  : "浏览器预览"}
            </span>
            <EndpointSelect
              endpoint={state().endpoint}
              endpoints={state().endpoints}
              disabled={!desktop || streaming()}
              onChange={(index) => void switchEndpoint(index)}
            />
          </div>
        </header>
        <Show when={error()}>
          <div class="global-error" role="alert">
            <AlertCircle size={17} />
            <span>{error()}</span>
            <button
              class="icon-button"
              title="关闭提示"
              aria-label="关闭提示"
              onClick={() => setError("")}
            >
              <X size={16} />
            </button>
          </div>
        </Show>
        <Show when={!desktop}>
          <div class="preview-banner">
            <Terminal size={15} />
            <span>浏览器预览 · 登录与 Agent 运行需要桌面客户端</span>
            <Show when={!preview()}>
              <button onClick={() => setPreview(true)}>
                查看工作区
                <ChevronRight size={14} />
              </button>
            </Show>
          </div>
        </Show>
        <Show
          when={!initializing()}
          fallback={
            <div class="loading-screen">
              <LoaderCircle class="spin" size={25} />
              <span>正在读取账户</span>
            </div>
          }
        >
          <Show
            when={view() === "settings"}
            fallback={
              <Show
                when={state().authenticated || preview()}
                fallback={
                  <div class="login-page">
                    <div class="login-content">
                      <div class="login-icon">
                        <Code2 size={27} />
                      </div>
                      <span class="eyebrow">YOUR NEXT WORKSPACE</span>
                      <h1>
                        {twoFactorToken() ? "两步验证" : "登录 Moyu Agent"}
                      </h1>
                      <p class="login-subtitle">
                        {twoFactorToken() ? email() : "Sub2API 账户"}
                      </p>
                      <form onSubmit={login}>
                        <Show
                          when={!twoFactorToken()}
                          fallback={
                            <>
                              <label for="totp">动态验证码</label>
                              <input
                                id="totp"
                                inputmode="numeric"
                                autocomplete="one-time-code"
                                pattern="[0-9]{6}"
                                required
                                placeholder="六位动态验证码"
                                value={totpCode()}
                                onInput={(event) =>
                                  setTotpCode(event.currentTarget.value)
                                }
                                disabled={busy()}
                              />
                            </>
                          }
                        >
                          <label for="email">邮箱</label>
                          <input
                            id="email"
                            type="email"
                            autocomplete="username"
                            required
                            placeholder="name@example.com"
                            value={email()}
                            onInput={(event) =>
                              setEmail(event.currentTarget.value)
                            }
                            disabled={busy()}
                          />
                          <label for="password">密码</label>
                          <div class="password-input">
                            <input
                              id="password"
                              type={passwordVisible() ? "text" : "password"}
                              autocomplete="current-password"
                              required
                              placeholder="输入账户密码"
                              value={password()}
                              onInput={(event) =>
                                setPassword(event.currentTarget.value)
                              }
                              disabled={busy()}
                            />
                            <button
                              type="button"
                              class="icon-button"
                              title={
                                passwordVisible() ? "隐藏密码" : "显示密码"
                              }
                              aria-label={
                                passwordVisible() ? "隐藏密码" : "显示密码"
                              }
                              onClick={() =>
                                setPasswordVisible((value) => !value)
                              }
                            >
                              {passwordVisible() ? (
                                <EyeOff size={17} />
                              ) : (
                                <Eye size={17} />
                              )}
                            </button>
                          </div>
                          <Show when={publicSettings().loginAgreementRequired}>
                            <label class="agreement">
                              <input
                                type="checkbox"
                                checked={agreement()}
                                onChange={(event) =>
                                  setAgreement(event.currentTarget.checked)
                                }
                                required
                              />
                              <span>
                                我已阅读并同意{" "}
                                <button type="button" class="agreement-link" onClick={() => agreementDialog?.showModal()}>
                                  服务协议
                                </button>
                              </span>
                            </label>
                          </Show>
                        </Show>
                        <button
                          class="primary-button login-button"
                          type="submit"
                          disabled={!desktop || busy()}
                        >
                          {busy() ? (
                            <LoaderCircle class="spin" size={17} />
                          ) : null}
                          {busy()
                            ? "正在登录"
                            : twoFactorToken()
                              ? "验证并登录"
                              : "登录"}
                          {!busy() && <ChevronRight size={17} />}
                        </button>
                      </form>
                      <Show when={twoFactorToken()}>
                        <button
                          class="text-button"
                          disabled={busy()}
                          onClick={() => {
                            setTwoFactorToken(null);
                            setTotpCode("");
                            setError("");
                          }}
                        >
                          返回登录
                        </button>
                      </Show>
                      <div class="login-security">
                        <ShieldCheck size={15} />
                        <span>账户凭证加密保存</span>
                      </div>
                      <Show when={publicSettings().registrationUrl}>
                        <a
                          class="register-link"
                          href={publicSettings().registrationUrl!}
                          target="_blank"
                          rel="noreferrer"
                        >
                          注册账户
                          <ChevronRight size={14} />
                        </a>
                      </Show>
                    </div>
                    <div class="login-footer">
                      <span>MOYU / WORKSPACE</span>
                      <span>v0.1.0</span>
                    </div>
                  </div>
                }
              >
                <div class="chat-pane">
                  <div
                    class="transcript"
                    ref={transcript}
                    onScroll={(event) => {
                      const node = event.currentTarget;
                      setNearBottom(
                        node.scrollHeight - node.scrollTop - node.clientHeight <
                          80,
                      );
                    }}
                  >
                    <Show
                      when={conversation().messages.length}
                      fallback={
                        <div class="empty-chat">
                          <div class="empty-mark">
                            <Terminal size={33} strokeWidth={1.4} />
                          </div>
                          <h2>今天，从哪里开始？</h2>
                          <span>
                            {state().config
                              ? `${state().config?.model} · ${
                                  folderName(state().config?.workspace) ||
                                  "默认工作区"
                                }`
                              : "工作区尚未配置"}
                          </span>
                          <Show when={state().authenticated}>
                            <button
                              class="text-button"
                              disabled={streaming() || busy()}
                              onClick={() => void chooseWorkspace()}
                            >
                              选择工作区
                              <ChevronRight size={15} />
                            </button>
                          </Show>
                        </div>
                      }
                    >
                      <div class="messages">
                        <For each={conversation().messages}>
                          {(message) => (
                            <article
                              classList={{
                                message: true,
                                "user-message": message.role === "user",
                                "assistant-message":
                                  message.role === "assistant",
                              }}
                            >
                              <div class="message-avatar">
                                {message.role === "assistant" ? (
                                  <Code2 size={16} />
                                ) : (
                                  state().user?.email.charAt(0).toUpperCase() ||
                                  "你"
                                )}
                              </div>
                              <div class="message-body">
                                <div class="message-meta">
                                  <strong>
                                    {message.role === "assistant"
                                      ? "Moyu"
                                      : "你"}
                                  </strong>
                                  <Show when={message.role === "assistant"}>
                                    <span>Agent</span>
                                  </Show>
                                  <Show when={message.state === "streaming"}>
                                    <span class="message-live">
                                      {streamStatus() || "处理中"}
                                    </span>
                                  </Show>
                                </div>
                                <Show
                                  when={message.text}
                                  fallback={
                                    <Show when={message.state === "streaming"}>
                                      <div class="typing-indicator">
                                        <span />
                                        <span />
                                        <span />
                                      </div>
                                    </Show>
                                  }
                                >
                                  <div class="message-content">
                                    {message.text}
                                    <Show when={message.state === "streaming"}>
                                      <span class="stream-cursor" />
                                    </Show>
                                  </div>
                                </Show>
                                <Show when={message.error}>
                                  <div class="message-error" role="alert">
                                    <AlertCircle size={15} />
                                    {message.error}
                                  </div>
                                </Show>
                                <Show when={message.state === "cancelled"}>
                                  <div class="message-cancelled">已停止</div>
                                </Show>
                                <Show
                                  when={
                                    message.role === "assistant" &&
                                    message.state !== "streaming"
                                  }
                                >
                                  <div class="message-tools">
                                    <Show when={message.text}>
                                      <button
                                        class="icon-button"
                                        title="复制回复"
                                        aria-label="复制回复"
                                        onClick={() => void copyMessage(message)}
                                      >
                                        {copiedId() === message.id ? (
                                          <Check size={14} />
                                        ) : (
                                          <Copy size={14} />
                                        )}
                                      </button>
                                    </Show>
                                    <Show
                                      when={
                                        message.state === "failed" ||
                                        message.state === "cancelled"
                                      }
                                    >
                                      <button
                                        class="icon-button"
                                        title="重试任务"
                                        aria-label="重试任务"
                                        disabled={streaming() || busy()}
                                        onClick={() => retry(message)}
                                      >
                                        <RotateCw size={14} />
                                      </button>
                                    </Show>
                                  </div>
                                </Show>
                              </div>
                            </article>
                          )}
                        </For>
                      </div>
                    </Show>
                  </div>
                  <div class="composer-area">
                    <Show when={!nearBottom()}>
                      <button
                        class="scroll-bottom icon-button"
                        title="跳到最新消息"
                        aria-label="跳到最新消息"
                        onClick={scrollToBottom}
                      >
                        <ArrowDown size={17} />
                      </button>
                    </Show>
                    <form
                      classList={{
                        composer: true,
                        "composer-disabled": !state().config,
                      }}
                      onSubmit={send}
                    >
                      <textarea
                        ref={composer}
                        aria-label="任务内容"
                        value={prompt()}
                        placeholder={
                          preview()
                            ? "桌面客户端准备就绪后开始任务"
                            : state().config
                              ? "给 Moyu 一个任务..."
                              : "请先配置工作区"
                        }
                        disabled={
                          !state().authenticated || !state().config || busy()
                        }
                        onInput={(event) =>
                          handleComposerInput(event.currentTarget)
                        }
                        onKeyDown={(event) => {
                          if (
                            event.key === "Enter" &&
                            !event.shiftKey &&
                            !event.isComposing
                          ) {
                            event.preventDefault();
                            void send();
                          }
                        }}
                        rows={2}
                      />
                      <div class="composer-toolbar">
                        <div class="composer-context">
                          <div class="workspace-picker">
                            <button
                              type="button"
                              class="composer-chip"
                              title={
                                state().config?.workspace || "选择工作区"
                              }
                              aria-label="选择工作区"
                              disabled={
                                !state().authenticated ||
                                streaming() ||
                                busy()
                              }
                              onClick={() => void chooseWorkspace()}
                            >
                              <Folder size={13} />
                              <span>
                                {folderName(state().config?.workspace) ||
                                  "选择工作区"}
                              </span>
                            </button>
                            <Show
                              when={
                                state().config ||
                                (state().recentWorkspaces?.length ?? 0) > 0
                              }
                            >
                              <button
                                type="button"
                                class="composer-chip-more"
                                title="工作区选项"
                                aria-label="工作区选项"
                                aria-expanded={workspaceMenu()}
                                disabled={
                                  !state().authenticated ||
                                  streaming() ||
                                  busy()
                                }
                                onClick={(event) => {
                                  event.stopPropagation();
                                  setWorkspaceMenu((open) => !open);
                                }}
                              >
                                <ChevronDown size={13} />
                              </button>
                            </Show>
                            <Show when={workspaceMenu()}>
                              <div class="workspace-menu" role="menu">
                                <For
                                  each={(state().recentWorkspaces ?? []).filter(
                                    (path) =>
                                      path !== state().config?.workspace,
                                  )}
                                >
                                  {(path) => (
                                    <button
                                      type="button"
                                      role="menuitem"
                                      title={path}
                                      onClick={() => void applyWorkspace(path)}
                                    >
                                      {folderName(path) || path}
                                    </button>
                                  )}
                                </For>
                                <button
                                  type="button"
                                  role="menuitem"
                                  disabled={!state().config}
                                  onClick={() => void revealWorkspace()}
                                >
                                  <FolderOpen size={13} />
                                  在资源管理器中打开
                                </button>
                              </div>
                            </Show>
                          </div>
                          <button
                            type="button"
                            class="composer-chip"
                            title="模型"
                            onClick={() => openSettings("model")}
                          >
                            <span
                              classList={{
                                "status-dot": true,
                                ready: !!state().config?.model,
                              }}
                            />
                            <span>{state().config?.model || "尚未配置模型"}</span>
                          </button>
                        </div>
                        <Show
                          when={streaming()}
                          fallback={
                            <button
                              type="submit"
                              class="send-button"
                              title="发送任务"
                              aria-label="发送任务"
                              disabled={
                                !prompt().trim() ||
                                !state().authenticated ||
                                !state().config ||
                                busy()
                              }
                            >
                              <ArrowUp size={19} />
                            </button>
                          }
                        >
                          <button
                            type="button"
                            class="stop-button"
                            title="停止任务"
                            aria-label="停止任务"
                            onClick={() => void stop()}
                          >
                            <Square size={13} fill="currentColor" />
                          </button>
                        </Show>
                      </div>
                    </form>
                    <div class="composer-footnote">
                      <span>
                        <span
                          classList={{
                            "status-dot": true,
                            ready: !!state().config,
                          }}
                        />
                        {streaming()
                          ? streamStatus() || "处理中"
                          : state().config
                            ? "准备就绪"
                            : "等待配置"}
                      </span>
                      <span>OpenCode</span>
                    </div>
                  </div>
                </div>
              </Show>
            }
          >
            <Settings
              state={state()}
              groups={groups()}
              busy={busy()}
              streaming={streaming()}
              tab={settingsTab()}
              pages={settingsMixins()}
              onTab={setSettingsTab}
              onEndpoint={switchEndpoint}
              onConfigured={(config) =>
                setState((current) => ({ ...current, config }))
              }
              onRefreshGroups={refreshGroups}
              onPluginsChange={() => void refreshMixins()}
            />
          </Show>
        </Show>
      </main>
      <Show when={mainMixins().length}>
        <div class="plugin-ui-layer">
          <For each={mainMixins()}>
            {(plugin) => (
              <Show
                when={!hiddenPluginUi().includes(plugin.id)}
                fallback={
                  <button
                    type="button"
                    class="plugin-ui-chip"
                    onClick={() =>
                      setHiddenPluginUi((ids) =>
                        ids.filter((id) => id !== plugin.id),
                      )
                    }
                  >
                    {plugin.name}
                  </button>
                }
              >
                <section class="plugin-ui-panel">
                  <header>
                    <strong>{plugin.name}</strong>
                    <span>Mixin</span>
                    <button
                      type="button"
                      class="icon-button"
                      title="收起"
                      aria-label={`收起 ${plugin.name}`}
                      onClick={() =>
                        setHiddenPluginUi((ids) => [...ids, plugin.id])
                      }
                    >
                      <X size={14} />
                    </button>
                  </header>
                  <iframe
                    title={plugin.name}
                    srcdoc={plugin.html}
                    sandbox="allow-scripts"
                  />
                </section>
              </Show>
            )}
          </For>
        </div>
      </Show>
      <dialog ref={agreementDialog} class="agreement-dialog" aria-labelledby="agreement-title">
        <div class="dialog-heading"><h2 id="agreement-title">服务协议</h2><button type="button" class="icon-button" aria-label="关闭协议" title="关闭协议" onClick={() => agreementDialog?.close()}><X size={18} /></button></div>
        <For each={publicSettings().agreementDocuments ?? []}>{document => <section><h3>{document.title}</h3><div class="agreement-content">{document.content}</div></section>}</For>
        <Show when={!publicSettings().agreementDocuments?.length}><p>暂未获取到协议正文，请重新切换线路后重试。</p></Show>
      </dialog>
    </div>
  );
}

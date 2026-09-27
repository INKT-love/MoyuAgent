import {
  createEffect,
  createMemo,
  createSignal,
  For,
  on,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { Channel } from "@tauri-apps/api/core";
import { Portal } from "solid-js/web";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  Eye,
  EyeOff,
  Archive,
  Ellipsis,
  Folder,
  FolderOpen,
  FolderPlus,
  GitBranch,
  LogOut,
  MessageSquare,
  PanelLeftOpen,
  Pencil,
  Pin,
  Plus,
  RotateCw,
  Settings2,
  Shield,
  ShieldCheck,
  Square,
  Terminal,
  X,
  LoaderCircle,
} from "lucide-solid";
import { ErrorNotice } from "./components/ErrorNotice";
import { ModelPicker } from "./components/ModelPicker";
import { PermissionPicker } from "./components/PermissionPicker";
import { ReasoningPicker } from "./components/ReasoningPicker";
import Settings, {
  EndpointSelect,
  pluginSettingsTab,
  type SettingsTab,
} from "./components/Settings";
import WindowControls from "./components/WindowControls";
import {
  modelChipLabel,
  parseReasoningEffort,
  reasoningEffortLabel,
  type ReasoningEffort,
} from "./lib/models";
import {
  parsePermissionMode,
  permissionChipLabel,
} from "./lib/permissions";
import { workspaceKey, workspaceLabel } from "./lib/workspaces";
import { localizeError } from "./lib/errors";
import {
  command,
  defaultModel,
  desktop,
  errorMessage,
  initialState,
  pickModel,
  type AppState,
  type ConfigSummary,
  type Endpoint,
  type Group,
  type GroupModel,
  type ConversationHistory,
  type LoginResult,
  type PublicSettings,
  type WorkspaceActionResult,
} from "./lib/api";
import {
  mixins,
  withBridge,
  type PluginMixin,
  type UiInjection,
} from "./lib/mixin";
import { createSessionStreams } from "./lib/session-streams";
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
  workspace: string;
  draft: string;
  pinned?: boolean;
  archived?: boolean;
}
const newConversation = (workspace = ""): Conversation => ({
  id: crypto.randomUUID(),
  title: "新任务",
  messages: [],
  workspace,
  draft: "",
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
  const [streamingIds, setStreamingIds] = createSignal<string[]>([]);
  const [streamStatuses, setStreamStatuses] = createSignal<Record<string, string>>(
    {},
  );
  const [copiedId, setCopiedId] = createSignal("");
  const [nearBottom, setNearBottom] = createSignal(true);
  const [workspaceBusy, setWorkspaceBusy] = createSignal(false);
  const [expandedWorkspaces, setExpandedWorkspaces] = createSignal<string[]>([]);
  const [modelNames, setModelNames] = createSignal<Record<string, string>>({});
  const [modelPicker, setModelPicker] = createSignal<{
    bottom: number;
    left: number;
  } | null>(null);
  const [reasoningPicker, setReasoningPicker] = createSignal<{
    bottom: number;
    left: number;
  } | null>(null);
  const [permissionPicker, setPermissionPicker] = createSignal<{
    bottom: number;
    left: number;
  } | null>(null);
  const chipLabel = createMemo(() => {
    const config = state().config;
    return modelChipLabel(
      config?.model,
      config?.model ? modelNames()[config.model] : undefined,
    );
  });
  const effortLabel = createMemo(() =>
    reasoningEffortLabel(state().config?.reasoningEffort),
  );
  const rememberModels = (list: GroupModel[]) => {
    if (!list.length) return;
    setModelNames((current) => {
      const next = { ...current };
      for (const item of list) next[item.id] = item.name;
      return next;
    });
  };
  const lastConversation = new Map<string, string>();
  const conversation = createMemo(
    () =>
      conversations().find((item) => item.id === activeConversationId()) ??
      conversations()[0],
  );
  const streams = createSessionStreams();
  const streaming = createMemo(() =>
    streamingIds().includes(conversation().id),
  );
  const runningCount = createMemo(() => streamingIds().length);
  const streamStatus = createMemo(() => {
    const raw = streamStatuses()[conversation().id] ?? "";
    return raw ? localizeError(raw).message : "";
  });
  const workspaceRunning = (path: string) =>
    conversations().some(
      (item) =>
        streamingIds().includes(item.id) &&
        workspaceKey(item.workspace) === workspaceKey(path),
    );
  const syncStreams = () => setStreamingIds(streams.ids());
  const setConversationStatus = (id: string, message: string) => {
    setStreamStatuses((current) => {
      if (!message) {
        if (!(id in current)) return current;
        const next = { ...current };
        delete next[id];
        return next;
      }
      return { ...current, [id]: message };
    });
  };
  const workspaces = createMemo(() => {
    if (!state().authenticated) return [];
    const paths = [
      ...(state().pinnedWorkspaces ?? []),
      ...(state().recentWorkspaces ?? []),
      ...conversations()
        .filter((item) => !item.archived)
        .map((item) => item.workspace),
      state().config?.workspace ?? "",
    ].filter(Boolean);
    return [...new Map(paths.map((path) => [workspaceKey(path), path])).values()];
  });
  const projectLabel = (path: string) =>
    workspaceLabel(path, state().workspaceLabels) || path;
  const workspacePinned = (path: string) =>
    (state().pinnedWorkspaces ?? []).some(
      (item) => workspaceKey(item) === workspaceKey(path),
    );
  const workspaceConversations = (path: string) =>
    conversations()
      .filter(
        (item) =>
          !item.archived &&
          item.messages.length &&
          workspaceKey(item.workspace) === workspaceKey(path),
      )
      .sort((a, b) => Number(!!b.pinned) - Number(!!a.pinned));
  const workspaceExpanded = (path: string) =>
    expandedWorkspaces().includes(workspaceKey(path));
  const expandWorkspace = (path: string) => {
    const key = workspaceKey(path);
    setExpandedWorkspaces((keys) =>
      keys.length === 1 && keys[0] === key ? keys : [key],
    );
  };
  let transcript: HTMLDivElement | undefined;
  let composer: HTMLTextAreaElement | undefined;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let scrollFrame: number | undefined;
  let disposed = false;
  let agreementDialog: HTMLDialogElement | undefined;
  let mixinObserver: MutationObserver | undefined;
  let mixinFrame: number | undefined;
  let historyReady = false;
  let persistTimer: ReturnType<typeof setTimeout> | undefined;
  let persistGeneration = 0;
  let persistInFlight = false;

  const patchConversation = (
    id: string,
    patch: (item: Conversation) => Conversation,
  ) =>
    setConversations((items) =>
      items.map((item) => (item.id === id ? patch(item) : item)),
    );
  const historyPayload = (): ConversationHistory => {
    const activeId = activeConversationId();
    return {
      activeId,
      conversations: conversations().map((item) =>
        item.id === activeId ? { ...item, draft: prompt() } : item,
      ),
    };
  };
  const flushPersist = async () => {
    if (!desktop || !historyReady || !state().authenticated) return;
    persistGeneration += 1;
    if (persistInFlight) return;
    persistInFlight = true;
    try {
      while (true) {
        const generation = persistGeneration;
        await command<void>("save_conversations", { history: historyPayload() });
        if (generation === persistGeneration) break;
      }
    } catch (err) {
      if (!disposed) setError(errorMessage(err));
    } finally {
      persistInFlight = false;
    }
  };
  const queuePersist = (immediate = false) => {
    if (!desktop || !historyReady || !state().authenticated) return;
    if (persistTimer !== undefined) {
      clearTimeout(persistTimer);
      persistTimer = undefined;
    }
    if (immediate) {
      void flushPersist();
      return;
    }
    persistTimer = setTimeout(() => {
      persistTimer = undefined;
      void flushPersist();
    }, 800);
  };
  const applyHistory = (history: ConversationHistory) => {
    const workspace = state().config?.workspace ?? "";
    const prepared: Conversation[] = (history.conversations ?? [])
      .filter((item) => item && typeof item.id === "string" && item.id)
      .map((item) => ({
        id: item.id,
        title: item.title || "新任务",
        messages: Array.isArray(item.messages) ? item.messages : [],
        sessionId: item.sessionId,
        workspace: item.workspace || "",
        draft: item.draft || "",
        pinned: !!item.pinned,
        archived: !!item.archived,
      }));
    lastConversation.clear();
    if (!prepared.length) {
      const fresh = newConversation(workspace);
      setConversations([fresh]);
      setActiveConversationId(fresh.id);
      setPrompt("");
      if (workspace) {
        lastConversation.set(workspaceKey(workspace), fresh.id);
        expandWorkspace(workspace);
      }
      return;
    }
    const sameWorkspace = (item: Conversation) =>
      workspaceKey(item.workspace || workspace) === workspaceKey(workspace);
    let active =
      prepared.find(
        (item) => item.id === history.activeId && (!workspace || sameWorkspace(item)),
      ) ??
      prepared.find((item) => !item.archived && sameWorkspace(item) && item.messages.length) ??
      prepared.find((item) => !item.archived && sameWorkspace(item));
    if (!active && workspace) {
      active = newConversation(workspace);
      prepared.unshift(active);
    }
    if (!active) active = prepared[0];
    if (!active) return;
    setConversations(prepared);
    setActiveConversationId(active.id);
    setPrompt(active.draft || "");
    for (const item of [...prepared].reverse()) {
      if (item.workspace)
        lastConversation.set(workspaceKey(item.workspace), item.id);
    }
    lastConversation.set(workspaceKey(active.workspace || workspace), active.id);
    if (active.workspace || workspace)
      expandWorkspace(active.workspace || workspace);
  };
  const loadHistory = async () => {
    if (!desktop || !state().authenticated) return;
    try {
      applyHistory(await command<ConversationHistory>("list_conversations"));
      historyReady = true;
    } catch (err) {
      if (!disposed) setError(errorMessage(err));
    }
  };
  const activateConversation = (next: Conversation) => {
    patchConversation(activeConversationId(), (item) => ({ ...item, draft: prompt() }));
    setActiveConversationId(next.id);
    setPrompt(next.draft);
    lastConversation.set(workspaceKey(next.workspace), next.id);
    setNearBottom(true);
    queuePersist(true);
  };
  const catalogGroupId = createMemo(() =>
    state().authenticated ? (state().config?.groupId ?? 0) : 0,
  );
  createEffect(
    on(catalogGroupId, (groupId) => {
      if (!desktop || !groupId) return;
      let cancelled = false;
      void command<GroupModel[]>("get_group_models", { groupId })
        .then((list) => {
          if (!cancelled) rememberModels(list);
        })
        .catch(() => undefined);
      onCleanup(() => {
        cancelled = true;
      });
    }),
  );
  createEffect(on(() => state().config?.workspace, (workspace) => {
    if (!workspace) return;
    expandWorkspace(workspace);
    const current = conversation();
    if (!current.workspace) {
      patchConversation(current.id, (item) => ({ ...item, workspace }));
      lastConversation.set(workspaceKey(workspace), current.id);
      queuePersist();
    } else if (workspaceKey(current.workspace) !== workspaceKey(workspace)) {
      const previousId = lastConversation.get(workspaceKey(workspace));
      let next = conversations().find((item) => item.id === previousId)
        ?? conversations().find((item) => workspaceKey(item.workspace) === workspaceKey(workspace));
      if (!next) {
        next = newConversation(workspace);
        setConversations((items) => [next!, ...items]);
      }
      activateConversation(next);
    }
  }));
  const refreshState = async () => {
    const next = await command<AppState>("get_app_state");
    if (!disposed) setState(next);
  };
  const refreshGroups = async () => {
    setBusy(true);
    try {
      setGroups(await command<Group[]>("get_groups", { refresh: true }));
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
      if (state().authenticated) {
        setGroups(await command<Group[]>("get_groups"));
        await loadHistory();
      }
      mixins.start();
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
    mixins.stop();
    mixins.reset();
    setPassword("");
    setTotpCode("");
    setTwoFactorToken(null);
    streams.disposeAll(true);
    syncStreams();
    if (copyTimer !== undefined) clearTimeout(copyTimer);
    if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame);
    if (persistTimer !== undefined) clearTimeout(persistTimer);
    if (historyReady && state().authenticated) void flushPersist();
  });

  let switchingEndpoint = false;
  const switchEndpoint = async (index: number) => {
    if (switchingEndpoint || state().endpoint.index === index)
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
  const [projectMenu, setProjectMenu] = createSignal<{
    path: string;
    top: number;
    left: number;
  } | null>(null);
  const [renamingPath, setRenamingPath] = createSignal<string | null>(null);
  const [renameValue, setRenameValue] = createSignal("");
  const closeMenus = () => {
    setWorkspaceMenu(false);
    setProjectMenu(null);
    setModelPicker(null);
    setReasoningPicker(null);
    setPermissionPicker(null);
  };
  const applyModelSelection = async (next: {
    groupId: number;
    model: string;
  }) => {
    if (streaming() || busy() || workspaceBusy() || !state().authenticated)
      return;
    const current = state().config;
    if (current && current.groupId === next.groupId && current.model === next.model) {
      return;
    }
    setError("");
    setBusy(true);
    try {
      const config = await command<ConfigSummary>("configure", {
        groupId: next.groupId,
        model: next.model,
        workspace: current?.workspace ?? "",
      });
      setState((current) => ({ ...current, config }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const applyReasoningEffort = async (effort: ReasoningEffort) => {
    if (streaming() || busy() || workspaceBusy() || !state().authenticated)
      return;
    const current = state().config;
    if (current && parseReasoningEffort(current.reasoningEffort) === effort) {
      return;
    }
    setError("");
    setBusy(true);
    try {
      const config = await command<ConfigSummary>("configure", {
        groupId: current?.groupId,
        model: current?.model,
        workspace: current?.workspace ?? "",
        reasoningEffort: effort,
      });
      setState((current) => ({ ...current, config }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const openModelPicker = (event: MouseEvent) => {
    event.stopPropagation();
    if (
      !state().authenticated ||
      streaming() ||
      busy() ||
      workspaceBusy()
    )
      return;
    const open = !!modelPicker();
    closeMenus();
    if (open) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const width = 280;
    let left = rect.right - width;
    if (left < 8) left = 8;
    if (left + width > window.innerWidth - 8) {
      left = Math.max(8, window.innerWidth - width - 8);
    }
    setModelPicker({
      bottom: window.innerHeight - rect.top + 6,
      left,
    });
  };
  const openReasoningPicker = (event: MouseEvent) => {
    event.stopPropagation();
    if (
      !state().authenticated ||
      streaming() ||
      busy() ||
      workspaceBusy()
    )
      return;
    const open = !!reasoningPicker();
    closeMenus();
    if (open) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const width = 248;
    let left = rect.right - width;
    if (left < 8) left = 8;
    if (left + width > window.innerWidth - 8) {
      left = Math.max(8, window.innerWidth - width - 8);
    }
    setReasoningPicker({
      bottom: window.innerHeight - rect.top + 6,
      left,
    });
  };
  const applyPermissionMode = async (mode: string) => {
    if (streaming() || busy() || workspaceBusy() || !state().authenticated)
      return;
    const current = state().config;
    const permissionMode = parsePermissionMode(mode);
    if (
      current &&
      parsePermissionMode(current.permissionMode) === permissionMode
    ) {
      return;
    }
    setError("");
    setBusy(true);
    try {
      const config = await command<ConfigSummary>("configure", {
        groupId: current?.groupId,
        model: current?.model,
        workspace: current?.workspace ?? "",
        permissionMode,
      });
      setState((current) => ({ ...current, config }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const openPermissionPicker = (event: MouseEvent) => {
    event.stopPropagation();
    if (
      !state().authenticated ||
      streaming() ||
      busy() ||
      workspaceBusy()
    )
      return;
    const open = !!permissionPicker();
    closeMenus();
    if (open) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const width = 340;
    let left = rect.left;
    if (left + width > window.innerWidth - 8) {
      left = Math.max(8, window.innerWidth - width - 8);
    }
    setPermissionPicker({
      bottom: window.innerHeight - rect.top + 6,
      left,
    });
  };
  const chooseWorkspace = async () => {
    if (!desktop || busy() || workspaceBusy() || !state().authenticated) return;
    closeMenus();
    setError("");
    setWorkspaceBusy(true);
    try {
      const config = await command<ConfigSummary | null>("choose_workspace");
      if (config) {
        setState((current) => ({ ...current, config }));
        setView("chat");
        setSidebarOpen(false);
      }
      await refreshState();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setWorkspaceBusy(false);
    }
  };
  const applyWorkspace = async (
    path: string,
    options?: { keepSidebar?: boolean; nested?: boolean },
  ) => {
    if (
      busy() ||
      (!options?.nested && workspaceBusy()) ||
      !state().authenticated
    )
      return false;
    closeMenus();
    expandWorkspace(path);
    if (workspaceKey(path) === workspaceKey(state().config?.workspace ?? "")) {
      setView("chat");
      if (!options?.keepSidebar) setSidebarOpen(false);
      return true;
    }
    setError("");
    if (!options?.nested) setWorkspaceBusy(true);
    try {
      const config = await command<ConfigSummary>("configure", {
        groupId: state().config?.groupId,
        model: state().config?.model,
        workspace: path,
      });
      setState((current) => ({ ...current, config }));
      setView("chat");
      if (!options?.keepSidebar) setSidebarOpen(false);
      await refreshState();
      return true;
    } catch (err) {
      setError(errorMessage(err));
      return false;
    } finally {
      if (!options?.nested) setWorkspaceBusy(false);
    }
  };
  const toggleWorkspace = async (path: string) => {
    const key = workspaceKey(path);
    if (workspaceExpanded(path)) {
      setExpandedWorkspaces((keys) => keys.filter((item) => item !== key));
      return;
    }
    expandWorkspace(path);
    if (workspaceKey(state().config?.workspace ?? "") !== key) {
      await applyWorkspace(path, { keepSidebar: true });
    }
  };
  const openConversation = async (item: Conversation) => {
    if (busy() || workspaceBusy()) return;
    if (!await applyWorkspace(item.workspace)) return;
    if (activeConversationId() !== item.id) activateConversation(item);
  };
  const togglePinned = (item: Conversation) => {
    patchConversation(item.id, (current) => ({
      ...current,
      pinned: !current.pinned,
    }));
    queuePersist(true);
  };
  const archiveConversation = (item: Conversation) => {
    const workspace = item.workspace;
    const wasActive = item.id === activeConversationId();
    setConversations((items) =>
      items.map((entry) =>
        entry.id === item.id
          ? { ...entry, archived: true, pinned: false }
          : entry,
      ),
    );
    if (!wasActive) {
      queuePersist(true);
      return;
    }
    const next =
      conversations().find(
        (entry) =>
          !entry.archived &&
          entry.messages.length &&
          workspaceKey(entry.workspace) === workspaceKey(workspace),
      ) ??
      conversations().find(
        (entry) =>
          !entry.archived &&
          workspaceKey(entry.workspace) === workspaceKey(workspace),
      );
    if (next) {
      activateConversation(next);
      return;
    }
    const fresh = newConversation(workspace);
    setConversations((items) => [fresh, ...items]);
    activateConversation(fresh);
  };
  const revealWorkspace = async () => {
    if (!desktop || !state().config) return;
    closeMenus();
    try {
      await command<void>("reveal_workspace");
    } catch (err) {
      setError(errorMessage(err));
    }
  };
  const applyWorkspaceLists = (result: WorkspaceActionResult) => {
    setState((current) => ({
      ...current,
      recentWorkspaces: result.recentWorkspaces,
      pinnedWorkspaces: result.pinnedWorkspaces,
      workspaceLabels: result.workspaceLabels,
    }));
  };
  const runWorkspaceAction = async (
    action: string,
    path: string,
    extra?: { name?: string },
  ) => {
    if (
      !desktop ||
      busy() ||
      workspaceBusy() ||
      !state().authenticated
    )
      return null;
    closeMenus();
    setError("");
    setWorkspaceBusy(true);
    try {
      const result = await command<WorkspaceActionResult>("workspace_action", {
        action,
        path,
        ...extra,
      });
      applyWorkspaceLists(result);
      if (result.switchTo != null) {
        await applyWorkspace(result.switchTo, {
          keepSidebar: true,
          nested: true,
        });
      }
      return result;
    } catch (err) {
      setError(errorMessage(err));
      return null;
    } finally {
      setWorkspaceBusy(false);
    }
  };
  const startNewIn = async (path: string) => {
    if (workspaceBusy() || busy()) return;
    closeMenus();
    if (!(await applyWorkspace(path, { keepSidebar: true }))) return;
    const current = conversation();
    if (
      workspaceKey(current.workspace) === workspaceKey(path) &&
      !current.messages.length &&
      !prompt().trim()
    ) {
      composer?.focus();
      return;
    }
    const fresh = newConversation(path);
    setConversations((items) => [fresh, ...items]);
    activateConversation(fresh);
    setView("chat");
    setPrompt("");
    setError("");
    expandWorkspace(path);
    queuePersist(true);
    composer?.focus();
  };
  const archiveWorkspaceChats = (path: string) => {
    closeMenus();
    const activeIn =
      workspaceKey(conversation().workspace) === workspaceKey(path);
    setConversations((items) =>
      items.map((entry) =>
        workspaceKey(entry.workspace) === workspaceKey(path)
          ? { ...entry, archived: true, pinned: false }
          : entry,
      ),
    );
    if (activeIn) {
      const fresh = newConversation(path);
      setConversations((items) => [fresh, ...items]);
      activateConversation(fresh);
    } else {
      queuePersist(true);
    }
  };
  const removeProject = async (path: string) => {
    const result = await runWorkspaceAction("remove", path);
    if (!result) return;
    setConversations((items) =>
      items.map((entry) =>
        workspaceKey(entry.workspace) === workspaceKey(path)
          ? { ...entry, archived: true, pinned: false }
          : entry,
      ),
    );
    queuePersist(true);
  };
  const beginRename = (path: string) => {
    closeMenus();
    setRenamingPath(path);
    setRenameValue(projectLabel(path));
  };
  const finishRename = async (path: string, commit: boolean) => {
    if (renamingPath() !== path) return;
    let name = renameValue();
    setRenamingPath(null);
    if (!commit) return;
    const folder = path.split(/[\\/]/).filter(Boolean).at(-1) || "";
    if (name.trim() === folder) name = "";
    await runWorkspaceAction("rename", path, { name });
  };
  const openProjectMenu = (event: MouseEvent, path: string) => {
    event.preventDefault();
    event.stopPropagation();
    if (projectMenu()?.path === path) {
      closeMenus();
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const width = 220;
    const estimated = 268;
    let left = rect.left;
    if (left + width > window.innerWidth - 8) {
      left = Math.max(8, window.innerWidth - width - 8);
    }
    let top = rect.bottom + 4;
    if (top + estimated > window.innerHeight - 8) {
      top = Math.max(8, rect.top - estimated - 4);
    }
    closeMenus();
    setProjectMenu({ path, top, left });
  };
  const onWindowKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    closeMenus();
    setRenamingPath(null);
  };
  onMount(() => {
    window.addEventListener("keydown", onWindowKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onWindowKeyDown));
  });
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
      const availableGroups = await command<Group[]>("get_groups");
      setGroups(availableGroups);
      if (!loggedIn.config && availableGroups.length) {
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
      await loadHistory();
    } catch (err) {
      setError(errorMessage(err));
      if (state().authenticated && !state().config) setView("settings");
    } finally {
      setBusy(false);
      void refreshState().catch(() => undefined);
    }
  };
  const logout = async () => {
    streams.disposeAll(true);
    syncStreams();
    setStreamStatuses({});
    setBusy(true);
    setError("");
    try {
      if (persistTimer !== undefined) {
        clearTimeout(persistTimer);
        persistTimer = undefined;
      }
      if (historyReady) await flushPersist();
      historyReady = false;
      await command<void>("logout");
      setState((current) => ({
        ...current,
        authenticated: false,
        user: null,
        config: null,
      }));
      setGroups([]);
      lastConversation.clear();
      setPrompt("");
      setView("chat");
      const fresh = newConversation();
      setConversations([fresh]);
      setActiveConversationId(fresh.id);
      setExpandedWorkspaces([]);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const startNew = () => {
    if (workspaceBusy()) return;
    if (conversation().messages.length || prompt().trim()) {
      const fresh = newConversation(state().config?.workspace);
      setConversations((items) => [fresh, ...items]);
      activateConversation(fresh);
    }
    const workspace = state().config?.workspace;
    if (workspace) expandWorkspace(workspace);
    setView("chat");
    setPrompt("");
    setSidebarOpen(false);
    setError("");
    queuePersist(true);
    composer?.focus();
  };
  const stop = async () => {
    const current = streams.get(conversation().id);
    if (!current) return;
    setConversationStatus(current.conversationId, "正在停止");
    try {
      await command<void>("cancel_stream", { requestId: current.requestId });
    } catch (err) {
      const message = errorMessage(err);
      setError(message);
      current.fail(message);
    }
  };
  const sendMessage = async (raw?: unknown) => {
    const text = String(raw ?? "").trim();
    if (
      !text ||
      streaming() ||
      busy() ||
      workspaceBusy() ||
      !state().authenticated ||
      !state().config
    )
      return;
    if (workspaceKey(conversation().workspace) !== workspaceKey(state().config!.workspace)) {
      setError("工作区尚未切换完成，请稍后重试。");
      return;
    }
    const requestId = crypto.randomUUID();
    const conversationId = conversation().id;
    const responseId = crypto.randomUUID();
    const sessionId = conversation().sessionId;
    const workspace = conversation().workspace || state().config?.workspace || "";
    setError("");
    setPrompt("");
    setConversationStatus(conversationId, "正在连接");
    setNearBottom(true);
    patchConversation(conversationId, (item) => ({
      ...item,
      draft: "",
      title: item.messages.length ? item.title : text.slice(0, 42),
      messages: [
        ...item.messages,
        { id: crypto.randomUUID(), role: "user", text },
        { id: responseId, role: "assistant", text: "", state: "streaming" },
      ],
    }));
    queuePersist(true);
    if (workspace) expandWorkspace(workspace);
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
        setConversationStatus(conversationId, "正在生成");
      },
      status: (message) => setConversationStatus(conversationId, message),
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
        channel.onmessage = () => undefined;
        streams.release(conversationId);
        syncStreams();
        setConversationStatus(conversationId, "");
        queuePersist(true);
        void refreshState().catch(() => undefined);
      },
      acknowledge: (sequence) =>
        command<void>("ack_stream", { requestId, sequence }),
      cancel: () => command<void>("cancel_stream", { requestId }),
    });
    channel.onmessage = (event) => controller.receive(event);
    streams.attach({
      conversationId,
      requestId,
      unlisten: (cancel = true) => {
        channel.onmessage = () => undefined;
        controller.unlisten(cancel);
      },
      fail: (message) => controller.fail(message),
    });
    syncStreams();
    try {
      await command<void>("start_stream", {
        requestId,
        prompt: text,
        sessionId,
        workspace,
        onEvent: channel,
      });
      void refreshState().catch(() => undefined);
    } catch (err) {
      controller.fail(errorMessage(err));
    }
  };
  const send = async (event?: SubmitEvent, override?: string) => {
    event?.preventDefault();
    try {
      await sendMessage(override ?? prompt());
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
    queuePersist();
  };

  return (
    <div class="app-shell" onClick={closeMenus}>
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
          data-tauri-drag-region
          onClick={(event) => {
            event.preventDefault();
            setView("chat");
          }}
        >
          <img src="/moyu.png" width="32" height="32" class="brand-mark" alt="" />
          <span>墨羽AGENT</span>
        </a>
        <button
          class="new-chat-button"
          onClick={startNew}
          disabled={workspaceBusy()}
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
          <div class="nav-section-label workspace-heading">
            <span>工作区</span>
            <button
              class="icon-button"
              title="添加工作区"
              aria-label="添加工作区"
              disabled={!state().authenticated || busy() || workspaceBusy()}
              onClick={() => void chooseWorkspace()}
            >
              <FolderPlus size={14} />
            </button>
          </div>
          <Show
            when={workspaces().length}
            fallback={<div class="history-empty">暂无工作区</div>}
          >
            <For each={workspaces()}>
              {(path) => (
                <div class="workspace-project">
                  <div
                    classList={{
                      "workspace-row-wrap": true,
                      selected:
                        workspaceKey(path) ===
                        workspaceKey(state().config?.workspace ?? ""),
                      expanded: workspaceExpanded(path),
                      "menu-open": projectMenu()?.path === path,
                      renaming: renamingPath() === path,
                    }}
                  >
                    <Show
                      when={renamingPath() === path}
                      fallback={
                        <button
                          type="button"
                          classList={{
                            "workspace-row": true,
                            selected:
                              workspaceKey(path) ===
                              workspaceKey(state().config?.workspace ?? ""),
                            expanded: workspaceExpanded(path),
                          }}
                          title={path}
                          aria-current={
                            workspaceKey(path) ===
                            workspaceKey(state().config?.workspace ?? "")
                              ? "true"
                              : undefined
                          }
                          aria-expanded={workspaceExpanded(path)}
                          disabled={busy() || workspaceBusy()}
                          onClick={() => void toggleWorkspace(path)}
                        >
                          <Show
                            when={workspaceExpanded(path)}
                            fallback={<Folder size={15} />}
                          >
                            <FolderOpen size={15} />
                          </Show>
                          <span class="workspace-name">{projectLabel(path)}</span>
                          <Show when={workspaceRunning(path)}>
                            <span class="workspace-running" title="进行中" />
                          </Show>
                          <Show when={workspacePinned(path)}>
                            <Pin size={11} class="workspace-pin-mark" />
                          </Show>
                        </button>
                      }
                    >
                      <div class="workspace-row">
                        <Folder size={15} />
                        <input
                          class="workspace-rename"
                          value={renameValue()}
                          aria-label="编辑工作区名称"
                          onClick={(event) => event.stopPropagation()}
                          onInput={(event) =>
                            setRenameValue(event.currentTarget.value)
                          }
                          onBlur={() => void finishRename(path, true)}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") {
                              event.preventDefault();
                              event.currentTarget.blur();
                            }
                            if (event.key === "Escape") {
                              event.preventDefault();
                              void finishRename(path, false);
                            }
                          }}
                          ref={(element) => {
                            queueMicrotask(() => {
                              element.focus();
                              element.select();
                            });
                          }}
                        />
                      </div>
                    </Show>
                    <div class="workspace-row-actions">
                      <button
                        type="button"
                        classList={{
                          "session-action": true,
                          active: projectMenu()?.path === path,
                        }}
                        title="更多选项"
                        aria-label="更多选项"
                        aria-haspopup="menu"
                        aria-expanded={projectMenu()?.path === path}
                        disabled={busy() || workspaceBusy()}
                        onClick={(event) => openProjectMenu(event, path)}
                      >
                        <Ellipsis size={13} />
                      </button>
                      <button
                        type="button"
                        class="session-action"
                        title="添加新会话"
                        aria-label="添加新会话"
                        disabled={busy() || workspaceBusy()}
                        onClick={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          void startNewIn(path);
                        }}
                      >
                        <Plus size={13} />
                      </button>
                    </div>
                  </div>
                  <Show when={workspaceExpanded(path) && workspaceConversations(path).length}>
                    <div class="workspace-conversations">
                      <For each={workspaceConversations(path)}>
                        {(item) => (
                          <div
                            classList={{
                              "session-row": true,
                              selected:
                                item.id === activeConversationId() &&
                                view() === "chat",
                              pinned: !!item.pinned,
                              running: streamingIds().includes(item.id),
                            }}
                          >
                            <button
                              type="button"
                              class="session-open"
                              title={item.title}
                              aria-busy={streamingIds().includes(item.id)}
                              disabled={busy() || workspaceBusy()}
                              onClick={() => void openConversation(item)}
                            >
                              <span>{item.title}</span>
                              <Show when={streamingIds().includes(item.id)}>
                                <span class="session-running" title="进行中" />
                              </Show>
                              <Show when={item.pinned}>
                                <Pin size={11} class="session-pin-mark" />
                              </Show>
                            </button>
                            <div class="session-actions">
                              <button
                                type="button"
                                classList={{
                                  "session-action": true,
                                  active: !!item.pinned,
                                }}
                                title={item.pinned ? "取消置顶" : "置顶"}
                                aria-label={item.pinned ? "取消置顶" : "置顶"}
                                aria-pressed={item.pinned ? "true" : "false"}
                                disabled={busy() || workspaceBusy()}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  togglePinned(item);
                                }}
                              >
                                <Pin
                                  size={13}
                                  fill={item.pinned ? "currentColor" : "none"}
                                />
                              </button>
                              <button
                                type="button"
                                class="session-action"
                                title="归档"
                                aria-label="归档"
                                disabled={busy() || workspaceBusy()}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  archiveConversation(item);
                                }}
                              >
                                <Archive size={13} />
                              </button>
                            </div>
                          </div>
                        )}
                      </For>
                    </div>
                  </Show>
                </div>
              )}
            </For>
          </Show>
        </div>
        <div class="sidebar-bottom">
          <div class="account-row">
            <span class="avatar">
              <img src="/moyu.png" width="29" height="29" alt="" />
            </span>
            <div>
              <strong>{state().user?.email || "尚未登录"}</strong>
              <span>
                {state().authenticated ? "Sub2API 账户" : "墨羽AGENT"}
              </span>
            </div>
            <Show when={state().authenticated}>
              <button
                class="icon-button"
                title="退出登录"
                aria-label="退出登录"
                disabled={busy()}
                onClick={() => void logout()}
              >
                <LogOut size={16} />
              </button>
            </Show>
          </div>
        </div>
      </aside>
      <Show when={projectMenu()}>
        {(menu) => (
          <Portal>
            <div
              class="workspace-menu project-menu"
              role="menu"
              style={{
                top: `${menu().top}px`,
                left: `${menu().left}px`,
              }}
              onClick={(event) => event.stopPropagation()}
            >
              <button
                type="button"
                role="menuitem"
                disabled={busy() || workspaceBusy()}
                onClick={() =>
                  void runWorkspaceAction(
                    workspacePinned(menu().path) ? "unpin" : "pin",
                    menu().path,
                  )
                }
              >
                <Pin
                  size={13}
                  fill={workspacePinned(menu().path) ? "currentColor" : "none"}
                />
                {workspacePinned(menu().path) ? "取消置顶" : "置顶"}
              </button>
              <button
                type="button"
                role="menuitem"
                disabled={busy() || workspaceBusy()}
                onClick={() => beginRename(menu().path)}
              >
                <Pencil size={13} />
                编辑
              </button>
              <button
                type="button"
                role="menuitem"
                disabled={busy() || workspaceBusy()}
                onClick={() => void runWorkspaceAction("reveal", menu().path)}
              >
                <FolderOpen size={13} />
                在资源管理器中打开
              </button>
              <button
                type="button"
                role="menuitem"
                disabled={busy() || workspaceBusy()}
                onClick={() =>
                  void runWorkspaceAction("create_worktree", menu().path)
                }
              >
                <GitBranch size={13} />
                创建永久工作树
              </button>
              <button
                type="button"
                role="menuitem"
                disabled={busy() || workspaceBusy()}
                onClick={() => archiveWorkspaceChats(menu().path)}
              >
                <Archive size={13} />
                归档聊天
              </button>
              <button
                type="button"
                role="menuitem"
                class="danger"
                disabled={busy() || workspaceBusy()}
                onClick={() => void removeProject(menu().path)}
              >
                <X size={13} />
                移除项目
              </button>
            </div>
          </Portal>
        )}
      </Show>
      <Show when={permissionPicker()}>
        {(picker) => (
          <Portal>
            <PermissionPicker
              mode={state().config?.permissionMode}
              style={picker()}
              onApply={(mode) => {
                closeMenus();
                void applyPermissionMode(mode);
              }}
            />
          </Portal>
        )}
      </Show>
      <Show when={reasoningPicker()}>
        {(picker) => (
          <Portal>
            <ReasoningPicker
              effort={state().config?.reasoningEffort}
              style={picker()}
              onApply={(effort) => {
                void applyReasoningEffort(effort);
              }}
            />
          </Portal>
        )}
      </Show>
      <Show when={modelPicker()}>
        {(picker) => (
          <Portal>
            <ModelPicker
              groups={groups()}
              groupId={state().config?.groupId ?? groups()[0]?.id ?? 0}
              model={state().config?.model ?? ""}
              style={picker()}
              onModels={rememberModels}
              onApply={(next) => {
                closeMenus();
                void applyModelSelection(next);
              }}
            />
          </Portal>
        )}
      </Show>
      <main class="main-pane">
        <header class="topbar" data-tauri-drag-region>
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
              disabled={!desktop || workspaceBusy()}
              onChange={(index) => void switchEndpoint(index)}
            />
            <WindowControls />
          </div>
        </header>
        <Show when={error()}>
          <div class="global-error" role="alert">
            <ErrorNotice error={error()} class="error-notice" />
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
                        <img src="/moyu.png" width="47" height="47" alt="" />
                      </div>
                      <span class="eyebrow">YOUR NEXT WORKSPACE</span>
                      <h1>
                        {twoFactorToken() ? "两步验证" : "登录墨羽AGENT"}
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
                          <h2>
                            {state().config ? <>在 <span class="empty-workspace" title={state().config?.workspace}>{projectLabel(state().config?.workspace ?? "") || "工作区"}</span> 中开始构建</> : "今天，想构建什么？"}
                          </h2>
                          <Show when={state().authenticated && !state().config}>
                            <button
                              class="text-button"
                              disabled={busy() || workspaceBusy()}
                              onClick={() => void chooseWorkspace()}
                            >
                              ��择工作区
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
                              aria-label={
                                message.role === "assistant"
                                  ? "助手回复"
                                  : "你的提示"
                              }
                            >
                              <div class="message-body">
                                <Show
                                  when={
                                    message.role === "assistant" &&
                                    message.state === "streaming"
                                  }
                                >
                                  <span class="message-live">
                                    {streamStatus() || "处理中"}
                                  </span>
                                </Show>
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
                                  <ErrorNotice error={message.error} />
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
                              ? "给墨羽一个任务..."
                              : "请先配置工作区"
                        }
                        disabled={
                          !state().authenticated || !state().config || busy() || workspaceBusy()
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
                                busy() || workspaceBusy()
                              }
                              onClick={() => void chooseWorkspace()}
                            >
                              <Folder size={13} />
                              <span>
                                {projectLabel(state().config?.workspace ?? "") ||
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
                                  busy() || workspaceBusy()
                                }
                                onClick={(event) => {
                                  event.stopPropagation();
                                  const open = workspaceMenu();
                                  closeMenus();
                                  if (!open) setWorkspaceMenu(true);
                                }}
                              >
                                <ChevronDown size={13} />
                              </button>
                            </Show>
                            <Show when={workspaceMenu()}>
                              <div class="workspace-menu" role="menu">
                                <For
                                  each={workspaces().filter(
                                    (path) =>
                                      workspaceKey(path) !== workspaceKey(state().config?.workspace ?? ""),
                                  )}
                                >
                                  {(path) => (
                                    <button
                                      type="button"
                                      role="menuitem"
                                      title={path}
                                      onClick={() => void applyWorkspace(path)}
                                    >
                                      {projectLabel(path)}
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
                            class="composer-chip permission-chip"
                            title={permissionChipLabel(
                              state().config?.permissionMode,
                            )}
                            aria-label="选择权限"
                            aria-haspopup="dialog"
                            aria-expanded={!!permissionPicker()}
                            disabled={
                              !state().authenticated ||
                              streaming() ||
                              busy() ||
                              workspaceBusy()
                            }
                            onClick={openPermissionPicker}
                          >
                            <Shield size={13} />
                            <span>
                              {permissionChipLabel(
                                state().config?.permissionMode,
                              )}
                            </span>
                          </button>
                        </div>
                        <div class="composer-actions">
                          <button
                            type="button"
                            class="composer-chip model-chip"
                            title={chipLabel()}
                            aria-label="选择模型"
                            aria-haspopup="dialog"
                            aria-expanded={!!modelPicker()}
                            disabled={
                              !state().authenticated ||
                              streaming() ||
                              busy() ||
                              workspaceBusy()
                            }
                            onClick={openModelPicker}
                          >
                            <span>{chipLabel()}</span>
                          </button>
                          <button
                            type="button"
                            class="composer-chip reasoning-chip"
                            title={`推理强度 ${effortLabel()}`}
                            aria-label="选择推理强度"
                            aria-haspopup="dialog"
                            aria-expanded={!!reasoningPicker()}
                            disabled={
                              !state().authenticated ||
                              streaming() ||
                              busy() ||
                              workspaceBusy()
                            }
                            onClick={openReasoningPicker}
                          >
                            <span>{effortLabel()}</span>
                          </button>
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
                                  busy() || workspaceBusy()
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
                      </div>
                    </form>
                    <div class="composer-footnote">
                      <span>
                        <span
                          classList={{
                            "status-dot": true,
                            ready: !!state().config && runningCount() === 0,
                            busy: runningCount() > 0,
                          }}
                        />
                        {workspaceBusy()
                          ? "正在切换工作区"
                          : streaming()
                            ? streamStatus() || "处理中"
                            : runningCount()
                              ? `${runningCount()} 个任务进行中`
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
              busy={busy() || workspaceBusy()}
              streaming={workspaceBusy()}
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
                    srcdoc={withBridge(plugin.html)}
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

export type MixinAt = "HEAD" | "RETURN" | "TAIL" | "WRAP";

export interface CallbackInfo<T = unknown> {
  args: unknown[];
  cancelled: boolean;
  returnValue: T | undefined;
  cancel(value?: T): void;
}

export interface UiInjection {
  id: string;
  name: string;
  html: string;
}

export interface PluginMixin {
  pluginId: string;
  pluginName: string;
  target?: string;
  select?: string;
  at: MixinAt | string;
  priority?: number;
  html?: string;
  script?: string;
}

type Injector = (ci: CallbackInfo) => void | Promise<void>;
type Wrapper = (
  original: (...args: unknown[]) => unknown,
  ...args: unknown[]
) => unknown;

interface DomMixin {
  pluginId: string;
  pluginName: string;
  select: string;
  at: MixinAt;
  html: string;
}

const asAt = (value: string): MixinAt => {
  const at = value.toUpperCase();
  if (at === "TAIL") return "RETURN";
  if (at === "HEAD" || at === "RETURN" || at === "WRAP") return at;
  return "RETURN";
};

export const isDocumentHtml = (html: string) =>
  /^\s*(<!doctype|<html)/i.test(html);

const inferredSelect = (mixin: PluginMixin) => {
  const select = mixin.select?.trim();
  if (select) return select;
  if (mixin.target === "ui.composer.toolbar") return ".composer-context";
  return "";
};

const isSettingsPage = (mixin: PluginMixin) => {
  const select = mixin.select?.trim() ?? "";
  const target = mixin.target ?? "";
  return (
    target === "ui.settings" ||
    select === ".settings-page" ||
    select === ".settings-tabs"
  );
};

export class MixinRuntime {
  private heads = new Map<string, { priority: number; fn: Injector }[]>();
  private returns = new Map<string, { priority: number; fn: Injector }[]>();
  private wraps = new Map<string, Wrapper[]>();
  private overlayList: UiInjection[] = [];
  private pageList: UiInjection[] = [];
  private domList: DomMixin[] = [];

  reset() {
    this.heads.clear();
    this.returns.clear();
    this.wraps.clear();
    this.overlayList = [];
    this.pageList = [];
    this.domList = [];
    this.clearDom();
  }

  register(mixin: PluginMixin) {
    const at = asAt(String(mixin.at ?? "RETURN"));
    const priority = mixin.priority ?? 1000;
    const html = mixin.html;
    const select = inferredSelect(mixin);
    if (html && isSettingsPage(mixin)) {
      this.pageList.push({
        id: mixin.pluginId,
        name: mixin.pluginName,
        html,
      });
    } else if (html && select) {
      this.domList.push({
        pluginId: mixin.pluginId,
        pluginName: mixin.pluginName,
        select,
        at,
        html,
      });
    } else if (html) {
      this.overlayList.push({
        id: mixin.pluginId,
        name: mixin.pluginName,
        html,
      });
    }
    const target = mixin.target ?? "";
    if (!mixin.script || !target || target.startsWith("ui.")) return;
    try {
      if (at === "WRAP") {
        const wrapper = new Function(
          `return (${mixin.script});`,
        )() as Wrapper;
        const list = this.wraps.get(target) ?? [];
        list.push(wrapper);
        this.wraps.set(target, list);
        return;
      }
      const injector = new Function(
        `return (${mixin.script});`,
      )() as Injector;
      const map = at === "HEAD" ? this.heads : this.returns;
      const list = map.get(target) ?? [];
      list.push({ priority, fn: injector });
      list.sort((left, right) => left.priority - right.priority);
      map.set(target, list);
    } catch {
      // Invalid mixin scripts fail closed and leave the target method intact.
    }
  }

  overlays(): UiInjection[] {
    return this.overlayList;
  }

  pages(): UiInjection[] {
    return this.pageList;
  }

  ui(target: string): UiInjection[] {
    if (target === "ui.main") return this.overlayList;
    if (target === "ui.settings") return this.pageList;
    return [];
  }

  clearDom(root: ParentNode | Document | undefined = globalThis.document) {
    if (!root) return;
    for (const node of [...root.querySelectorAll("[data-mixin]")]) {
      node.remove();
    }
  }

  applyDom(root: ParentNode | Document | undefined = globalThis.document) {
    if (!root) return;
    this.domList.forEach((mixin, mixinIndex) => {
      let nodes: NodeListOf<Element>;
      try {
        nodes = root.querySelectorAll(mixin.select);
      } catch {
        return;
      }
      nodes.forEach((node, index) => {
        if (node instanceof HTMLElement && node.dataset.mixin) return;
        const key = `${mixin.pluginId}-${mixinIndex}-${index}`;
        if (node.querySelector(`[data-mixin="${key}"]`)) return;
        const holder = document.createElement("div");
        holder.dataset.mixin = key;
        holder.className = "mixin-slot";
        if (isDocumentHtml(mixin.html)) {
          const frame = document.createElement("iframe");
          frame.title = mixin.pluginName;
          frame.srcdoc = mixin.html;
          frame.setAttribute("sandbox", "allow-scripts");
          holder.appendChild(frame);
        } else {
          holder.innerHTML = mixin.html;
        }
        if (mixin.at === "HEAD") node.insertBefore(holder, node.firstChild);
        else node.appendChild(holder);
      });
    });
  }

  async invoke<T>(
    target: string,
    original: (...args: unknown[]) => T | Promise<T>,
    args: unknown[],
  ): Promise<T | undefined> {
    const ci: CallbackInfo<T> = {
      args: [...args],
      cancelled: false,
      returnValue: undefined,
      cancel(value?: T) {
        this.cancelled = true;
        this.returnValue = value;
      },
    };
    for (const injector of this.heads.get(target) ?? []) {
      await injector.fn(ci);
    }
    if (ci.cancelled) return ci.returnValue;
    type Fn = (...args: unknown[]) => unknown;
    const stacked = (this.wraps.get(target) ?? []).reduceRight<Fn>(
      (next, wrap) =>
        (...inner: unknown[]) =>
          wrap(next, ...inner),
      original as Fn,
    );
    ci.returnValue = (await stacked(...ci.args)) as T;
    for (const injector of this.returns.get(target) ?? []) {
      await injector.fn(ci);
    }
    return ci.returnValue;
  }
}

export const mixins = new MixinRuntime();

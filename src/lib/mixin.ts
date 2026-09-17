import { command } from "./api";

export type MixinAt = "HEAD" | "RETURN" | "TAIL" | "WRAP";

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

const asAt = (value: string): MixinAt => {
  const at = value.toUpperCase();
  if (at === "TAIL") return "RETURN";
  if (at === "HEAD" || at === "RETURN" || at === "WRAP") return at;
  return "RETURN";
};

export const isDocumentHtml = (html: string) =>
  /^\s*(<!doctype|<html)/i.test(html);

const isSettingsPage = (mixin: PluginMixin) => {
  const select = mixin.select?.trim() ?? "";
  return select === ".settings-page" || select === ".settings-tabs";
};

const BRIDGE = `(function(){
  if (window.Moyu) return;
  var pending = {};
  window.addEventListener("message", function(event) {
    var data = event.data;
    if (!data || data.source !== "moyu-mixin-result" || !data.id) return;
    var job = pending[data.id];
    if (!job) return;
    delete pending[data.id];
    if (data.ok) job.resolve(data.result);
    else job.reject(new Error(data.error || "mixin failed"));
  });
  window.Moyu = {
    invoke: function(action, args) {
      return new Promise(function(resolve, reject) {
        var id = Math.random().toString(36).slice(2) + String(Date.now());
        pending[id] = { resolve: resolve, reject: reject };
        parent.postMessage({ source: "moyu-mixin", id: id, action: action, args: args || {} }, "*");
      });
    },
    state: function() { return window.Moyu.invoke("get_app_state"); }
  };
})();`;

export function withBridge(html: string) {
  if (!html || html.includes("moyu-mixin-result")) return html;
  const tag = `<script>${BRIDGE}</script>`;
  if (/<head[^>]*>/i.test(html))
    return html.replace(/<head[^>]*>/i, (head) => `${head}${tag}`);
  if (/<html[^>]*>/i.test(html))
    return html.replace(/<html[^>]*>/i, (root) => `${root}<head>${tag}</head>`);
  if (isDocumentHtml(html)) return `<!doctype html><head>${tag}</head>${html}`;
  return html;
}

interface DomMixin {
  pluginId: string;
  pluginName: string;
  select: string;
  at: MixinAt;
  html?: string;
  script?: string;
}

export class MixinRuntime {
  private overlayList: UiInjection[] = [];
  private pageList: UiInjection[] = [];
  private domList: DomMixin[] = [];
  private listening = false;

  private onMessage = (event: MessageEvent) => {
    const data = event.data;
    if (!data || data.source !== "moyu-mixin") return;
    if (typeof data.id !== "string" || typeof data.action !== "string") return;
    const args =
      data.args && typeof data.args === "object" && !Array.isArray(data.args)
        ? data.args
        : {};
    const source = event.source;
    const reply = (payload: Record<string, unknown>) => {
      if (source && "postMessage" in source) {
        (source as Window).postMessage(
          { source: "moyu-mixin-result", id: data.id, ...payload },
          "*",
        );
      }
    };
    void command(data.action, args)
      .then((result) => reply({ ok: true, result }))
      .catch((error: unknown) =>
        reply({
          ok: false,
          error: error instanceof Error ? error.message : String(error),
        }),
      );
  };

  start() {
    if (this.listening || typeof window === "undefined") return;
    window.addEventListener("message", this.onMessage);
    this.listening = true;
  }

  stop() {
    if (!this.listening || typeof window === "undefined") return;
    window.removeEventListener("message", this.onMessage);
    this.listening = false;
  }

  reset() {
    this.overlayList = [];
    this.pageList = [];
    this.domList = [];
    this.clearDom();
  }

  register(mixin: PluginMixin) {
    const at = asAt(String(mixin.at ?? "RETURN"));
    const html = mixin.html;
    const select = mixin.select?.trim() ?? "";
    if (html && isSettingsPage(mixin)) {
      this.pageList.push({
        id: mixin.pluginId,
        name: mixin.pluginName,
        html: withBridge(html),
      });
    } else if (select && (html || mixin.script)) {
      this.domList.push({
        pluginId: mixin.pluginId,
        pluginName: mixin.pluginName,
        select,
        at,
        html,
        script: mixin.script,
      });
    } else if (html) {
      this.overlayList.push({
        id: mixin.pluginId,
        name: mixin.pluginName,
        html: withBridge(html),
      });
    }
  }

  overlays(): UiInjection[] {
    return this.overlayList;
  }

  pages(): UiInjection[] {
    return this.pageList;
  }

  clearDom(root: ParentNode | Document | undefined = globalThis.document) {
    if (!root) return;
    for (const node of [...root.querySelectorAll("[data-mixin]")]) {
      node.remove();
    }
    for (const node of [
      ...root.querySelectorAll("[data-mixin-script]"),
    ] as HTMLElement[]) {
      delete node.dataset.mixinScript;
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
        if (mixin.html) {
          if (node.querySelector(`[data-mixin="${key}"]`)) return;
          const holder = document.createElement("div");
          holder.dataset.mixin = key;
          holder.className = "mixin-slot";
          if (isDocumentHtml(mixin.html)) {
            const frame = document.createElement("iframe");
            frame.title = mixin.pluginName;
            frame.srcdoc = withBridge(mixin.html);
            frame.setAttribute("sandbox", "allow-scripts");
            holder.appendChild(frame);
          } else {
            holder.innerHTML = mixin.html;
          }
          if (mixin.at === "HEAD") node.insertBefore(holder, node.firstChild);
          else node.appendChild(holder);
          return;
        }
        if (mixin.script && node instanceof HTMLElement) {
          if (node.dataset.mixinScript === key) return;
          node.dataset.mixinScript = key;
          try {
            const compiled = new Function(`return (${mixin.script});`)();
            if (typeof compiled === "function") compiled(node);
          } catch {
            // DOM scripts fail closed.
          }
        }
      });
    });
  }
}

export const mixins = new MixinRuntime();

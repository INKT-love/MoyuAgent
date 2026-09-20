import { For, Show } from "solid-js";
import { Check } from "lucide-solid";
import {
  parsePermissionMode,
  PERMISSION_MODES,
  type PermissionMode,
} from "../lib/permissions";

export function PermissionPicker(props: {
  mode?: string;
  style: { bottom: number; left: number };
  onApply: (mode: PermissionMode) => void;
}) {
  const selected = () => parsePermissionMode(props.mode);
  return (
    <div
      class="workspace-menu project-menu permission-picker"
      role="dialog"
      aria-label="应如何批准墨羽操作？"
      style={{
        bottom: `${props.style.bottom}px`,
        left: `${props.style.left}px`,
      }}
      onClick={(event) => event.stopPropagation()}
    >
      <div class="permission-picker-head">
        <span>应如何批准墨羽操作？</span>
        <span
          class="permission-picker-more"
          title="这些选项会写入 OpenCode 权限：是否允许上网，以及能否读写工作区以外的文件。"
        >
          了解更多
        </span>
      </div>
      <div class="permission-picker-list">
        <For each={PERMISSION_MODES}>
          {(item) => (
            <button
              type="button"
              classList={{ selected: item.id === selected() }}
              onClick={() => props.onApply(item.id)}
            >
              <span class="permission-picker-copy">
                <strong>{item.label}</strong>
                <small>{item.description}</small>
              </span>
              <Show when={item.id === selected()}>
                <Check size={16} />
              </Show>
            </button>
          )}
        </For>
      </div>
    </div>
  );
}

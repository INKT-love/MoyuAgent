import { Show } from "solid-js";
import { Check, LoaderCircle, Save } from "lucide-solid";

export function SaveBar(props: {
  saving: boolean;
  success: boolean;
  dirty: boolean;
  disabled: boolean;
}) {
  return (
    <div class="settings-footer">
      <span
        classList={{
          "save-result": true,
          success: props.success && !props.dirty,
          pending: props.dirty,
        }}
      >
        <Show when={props.success && !props.dirty}>
          <Check size={15} />
          配置已写入
        </Show>
        <Show when={props.dirty}>未保存的更改</Show>
      </span>
      <button
        class="primary-button"
        type="submit"
        disabled={props.disabled}
      >
        {props.saving ? (
          <LoaderCircle size={16} class="spin" />
        ) : (
          <Save size={16} />
        )}
        {props.saving ? "正在配置" : "一键配置"}
      </button>
    </div>
  );
}

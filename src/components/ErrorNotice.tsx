import { createSignal, Show } from "solid-js";
import { AlertCircle, ChevronDown, ChevronRight } from "lucide-solid";
import { localizeError } from "../lib/errors";

export function ErrorNotice(props: {
  error?: string | null;
  class?: string;
}) {
  const [open, setOpen] = createSignal(false);
  const localized = () => localizeError(props.error);
  return (
    <Show when={props.error}>
      <div class={props.class ?? "message-error"} role="alert">
        <AlertCircle size={15} />
        <div class="error-copy">
          <div class="error-headline">
            <span>{localized().message}</span>
            <Show when={localized().original}>
              <button
                type="button"
                class="error-original-toggle"
                aria-expanded={open()}
                onClick={(event) => {
                  event.stopPropagation();
                  setOpen((current) => !current);
                }}
              >
                {open() ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                {open() ? "隐藏原始报错" : "显示原始报错"}
              </button>
            </Show>
          </div>
          <Show when={open() && localized().original}>
            {(original) => <pre class="error-original">{original()}</pre>}
          </Show>
        </div>
      </div>
    </Show>
  );
}

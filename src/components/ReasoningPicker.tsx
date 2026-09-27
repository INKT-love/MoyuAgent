import { createEffect, createSignal, For } from "solid-js";
import {
  parseReasoningEffort,
  REASONING_EFFORT_MAX,
  REASONING_EFFORTS,
  reasoningEffortAt,
  reasoningEffortIndex,
  reasoningEffortLabel,
  type ReasoningEffort,
} from "../lib/models";

export function ReasoningPicker(props: {
  effort?: string;
  style: { bottom: number; left: number };
  onApply: (effort: ReasoningEffort) => void;
}) {
  const [index, setIndex] = createSignal(reasoningEffortIndex(props.effort));

  createEffect(() => {
    setIndex(reasoningEffortIndex(props.effort));
  });

  const selected = () => parseReasoningEffort(props.effort);
  const preview = () => reasoningEffortAt(index());
  const percent = () =>
    REASONING_EFFORT_MAX === 0 ? 0 : (index() / REASONING_EFFORT_MAX) * 100;

  const commit = (next: number) => {
    const effort = reasoningEffortAt(next);
    setIndex(reasoningEffortIndex(effort));
    if (effort === selected()) return;
    props.onApply(effort);
  };

  return (
    <div
      class="workspace-menu project-menu reasoning-picker"
      role="dialog"
      aria-label="推理强度"
      style={{
        bottom: `${props.style.bottom}px`,
        left: `${props.style.left}px`,
      }}
      onClick={(event) => event.stopPropagation()}
    >
      <div class="reasoning-picker-head">
        <span>推理强度</span>
        <strong>{reasoningEffortLabel(preview())}</strong>
      </div>
      <label class="reasoning-slider">
        <span class="sr-only">推理强度</span>
        <input
          type="range"
          min="0"
          max={REASONING_EFFORT_MAX}
          step="1"
          value={index()}
          aria-valuemin={0}
          aria-valuemax={REASONING_EFFORT_MAX}
          aria-valuenow={index()}
          aria-valuetext={reasoningEffortLabel(preview())}
          style={{ "--progress": `${percent()}%` }}
          onInput={(event) => setIndex(Number(event.currentTarget.value))}
          onPointerUp={(event) =>
            commit(Number((event.currentTarget as HTMLInputElement).value))
          }
          onKeyUp={(event) =>
            commit(Number((event.currentTarget as HTMLInputElement).value))
          }
          onChange={(event) => commit(Number(event.currentTarget.value))}
        />
      </label>
      <div class="reasoning-picker-ticks" aria-hidden="true">
        <For each={REASONING_EFFORTS}>{(item) => <span>{item.label}</span>}</For>
      </div>
    </div>
  );
}

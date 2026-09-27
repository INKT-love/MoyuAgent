import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { Check, ChevronLeft, LoaderCircle } from "lucide-solid";
import type { Group, GroupModel } from "../lib/api";
import { command, errorMessage, pickModel } from "../lib/api";
import { localizeError } from "../lib/errors";

export function ModelPicker(props: {
  groups: Group[];
  groupId: number;
  model: string;
  style: { bottom: number; left: number };
  onModels: (models: GroupModel[]) => void;
  onApply: (next: { groupId: number; model: string }) => void;
}) {
  const [step, setStep] = createSignal<"group" | "model">("group");
  const [groupId, setGroupId] = createSignal(props.groupId);
  const [model, setModel] = createSignal(props.model);
  const [models, setModels] = createSignal<GroupModel[]>([]);
  const [query, setQuery] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal("");
  let fetchedGroupId = 0;

  createEffect(() => {
    if (step() !== "model") return;
    const id = groupId();
    if (!id) {
      fetchedGroupId = 0;
      setModels([]);
      setLoading(false);
      return;
    }
    if (fetchedGroupId === id) return;
    let cancelled = false;
    setLoading(true);
    setError("");
    void command<GroupModel[]>("get_group_models", { groupId: id })
      .then((list) => {
        if (cancelled) return;
        fetchedGroupId = id;
        setModels(list);
        props.onModels(list);
        setModel((current) =>
          pickModel(list, current || (id === props.groupId ? props.model : "")),
        );
      })
      .catch((err) => {
        if (cancelled) return;
        fetchedGroupId = 0;
        setModels([]);
        setError(errorMessage(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    onCleanup(() => {
      cancelled = true;
    });
  });

  const visibleModels = createMemo(() => {
    const selected = model();
    const needle = query().trim().toLowerCase();
    const list = models();
    if (!needle) return list;
    return list.filter(
      (item) =>
        item.id === selected ||
        item.id.toLowerCase().includes(needle) ||
        item.name.toLowerCase().includes(needle),
    );
  });

  const title = () => (step() === "group" ? "选择分组" : "选择模型");

  const back = () => {
    if (step() === "model") setStep("group");
  };

  const chooseGroup = (group: Group) => {
    setGroupId(group.id);
    setModel(group.id === props.groupId ? props.model : "");
    setQuery("");
    setError("");
    if (fetchedGroupId !== group.id) setModels([]);
    setStep("model");
  };

  const chooseModel = (item: GroupModel) => {
    if (!groupId()) return;
    setModel(item.id);
    props.onApply({
      groupId: groupId(),
      model: item.id,
    });
  };

  return (
    <div
      class="workspace-menu project-menu model-picker"
      role="dialog"
      aria-label={title()}
      style={{
        bottom: `${props.style.bottom}px`,
        left: `${props.style.left}px`,
      }}
      onClick={(event) => event.stopPropagation()}
    >
      <div class="model-picker-head">
        <Show when={step() !== "group"}>
          <button
            type="button"
            class="model-picker-back"
            aria-label="返回"
            onClick={back}
          >
            <ChevronLeft size={14} />
          </button>
        </Show>
        <span>{title()}</span>
      </div>
      <Show when={step() === "group"}>
        <div class="model-picker-list">
          <Show
            when={props.groups.length}
            fallback={<p class="model-picker-empty">暂无可用分组</p>}
          >
            <For each={props.groups}>
              {(group) => (
                <button
                  type="button"
                  classList={{ selected: group.id === groupId() }}
                  onClick={() => chooseGroup(group)}
                >
                  <span>{group.name}</span>
                  <Show when={group.id === groupId()}>
                    <Check size={13} />
                  </Show>
                </button>
              )}
            </For>
          </Show>
        </div>
      </Show>
      <Show when={step() === "model"}>
        <Show when={models().length > 8}>
          <input
            class="model-picker-search"
            type="search"
            value={query()}
            placeholder="筛选模型名称或 ID"
            onInput={(event) => setQuery(event.currentTarget.value)}
          />
        </Show>
        <div class="model-picker-list">
          <Show when={loading() && !models().length}>
            <p class="model-picker-empty">
              <LoaderCircle size={13} class="spin" />
              正在加载模型
            </p>
          </Show>
          <Show when={!loading() && error()}>
            <p class="model-picker-error">{localizeError(error()).message}</p>
          </Show>
          <Show when={!loading() && !error() && !visibleModels().length}>
            <p class="model-picker-empty">暂无可用模型</p>
          </Show>
          <For each={visibleModels()}>
            {(item) => (
              <button
                type="button"
                classList={{ selected: item.id === model() }}
                title={item.id}
                onClick={() => chooseModel(item)}
              >
                <span>{item.name || item.id}</span>
                <Show when={item.id === model()}>
                  <Check size={13} />
                </Show>
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

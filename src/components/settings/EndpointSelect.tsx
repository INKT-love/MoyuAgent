import { createEffect, For } from "solid-js";
import { ChevronDown, Globe2 } from "lucide-solid";
import type { Endpoint } from "../../lib/api";
import { ENDPOINTS } from "../../lib/api";

export function EndpointSelect(props: {
  id?: string;
  endpoint: Endpoint;
  endpoints: Endpoint[];
  disabled?: boolean;
  onChange: (index: number) => void;
}) {
  let selectRef: HTMLSelectElement | undefined;
  const value = () => String(props.endpoint.index);
  const applyValue = (node: HTMLSelectElement | undefined) => {
    const next = value();
    if (node && node.value !== next) node.value = next;
  };
  createEffect(() => applyValue(selectRef));
  return (
    <div class="select-wrap endpoint-select">
      <Globe2 size={15} aria-hidden="true" />
      <select
        ref={(node) => {
          selectRef = node;
          applyValue(node);
        }}
        id={props.id}
        aria-label="API 线路"
        value={value()}
        disabled={props.disabled}
        onChange={(event) => props.onChange(Number(event.currentTarget.value))}
      >
        <For each={props.endpoints}>
          {(endpoint) => (
            <option value={String(endpoint.index)}>
              {ENDPOINTS[endpoint.index]?.name ?? endpoint.name}
            </option>
          )}
        </For>
      </select>
      <ChevronDown size={14} aria-hidden="true" />
    </div>
  );
}

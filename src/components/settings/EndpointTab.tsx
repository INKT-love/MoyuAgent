import { Globe2 } from "lucide-solid";
import type { AppState } from "../../lib/api";
import { desktop } from "../../lib/api";
import { EndpointSelect } from "./EndpointSelect";

export function EndpointTab(props: {
  state: AppState;
  streaming: boolean;
  onEndpoint: (index: number) => Promise<void>;
}) {
  return (
    <section
      class="settings-card"
      role="tabpanel"
      id="settings-panel-endpoint"
      aria-labelledby="settings-tab-endpoint"
    >
      <div class="section-heading">
        <Globe2 size={18} />
        <h2>API 线路</h2>
      </div>
      <p class="settings-copy">切换立即生效，后续请求走所选线路。</p>
      <div class="setting-field">
        <label for="endpoint-settings">当前线路</label>
        <EndpointSelect
          id="endpoint-settings"
          endpoint={props.state.endpoint}
          endpoints={props.state.endpoints}
          disabled={!desktop || props.streaming}
          onChange={(index) => void props.onEndpoint(index)}
        />
        <code class="setting-hint">{props.state.endpoint.baseUrl}</code>
      </div>
    </section>
  );
}

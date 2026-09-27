export interface LocalizedError {
  message: string;
  original?: string;
}

const TRANSLATIONS: Array<[RegExp, string]> = [
  [/^Sign in before configuring OpenCode$/i, "请先登录后再配置模型。"],
  [/^Sign in before starting the agent$/i, "请先登录后再开始任务。"],
  [/^Sign in before continuing$/i, "请先登录后再继续。"],
  [/^Sign in before listing groups$/i, "请先登录后再查看分组。"],
  [/^Sign in before listing models$/i, "请先登录后再查看模型。"],
  [/^Sign in before listing conversations$/i, "请先登录后再查看对话。"],
  [/^Sign in before saving conversations$/i, "请先登录后再保存对话。"],
  [/^Sign in before .+$/i, "请先登录后再继续。"],
  [
    /^Unable to create an OpenCode session(?::\s*.+)?$/i,
    "无法创建本地会话。",
  ],
  [
    /^Unable to reach the local OpenCode server$/i,
    "无法连接本地引擎，请稍后重试。",
  ],
  [
    /^Unable to subscribe to OpenCode events(?::\s*.+)?$/i,
    "无法订阅本地引擎事件，请稍后重试。",
  ],
  [
    /^Unable to send the prompt to OpenCode(?::\s*.+)?$/i,
    "无法向本地引擎发送任务，请稍后重试。",
  ],
  [
    /^OpenCode request failed(?::\s*.+)?$/i,
    "本地引擎请求失败，请稍后重试。",
  ],
  [
    /^OpenCode rejected the previous session; start a new conversation$/i,
    "上一轮会话已失效，请新开对话后再试。",
  ],
  [
    /^OpenCode returned an invalid session$/i,
    "本地引擎返回了无效会话，请重试。",
  ],
  [/^OpenCode is not running$/i, "本地引擎尚未启动，请稍后重试。"],
  [
    /^The bundled OpenCode executable was not found$/i,
    "未找到内置引擎，请重新安装墨羽AGENT。",
  ],
  [
    /^Unable to start the bundled OpenCode executable$/i,
    "无法启动内置引擎，请重新安装后重试。",
  ],
  [
    /^Unable to start OpenCode$/i,
    "无法启动本地引擎，请稍后重试。",
  ],
  [/^OpenCode startup was cancelled$/i, "本地引擎启动已取消。"],
  [
    /^OpenCode configuration is missing\. Save your model settings and retry\.$/i,
    "缺少引擎配置。请先保存模型设置后再试。",
  ],
  [
    /^OpenCode started but did not become healthy$/i,
    "本地引擎已启动，但尚未就绪，请稍后重试。",
  ],
  [
    /^OpenCode did not report a local listening address$/i,
    "本地引擎没有返回监听地址，请重试。",
  ],
  [
    /^OpenCode exited before the local server was ready$/i,
    "本地引擎在就绪前退出，请重试。",
  ],
  [
    /^OpenCode closed output before the local server was ready$/i,
    "本地引擎在就绪前关闭了输出，请重试。",
  ],
  [
    /^OpenCode did not provide server output$/i,
    "本地引擎没有返回启动输出，请重试。",
  ],
  [
    /^Unable to read the OpenCode server address$/i,
    "无法读取本地引擎地址，请重试。",
  ],
  [
    /^Unable to isolate the agent process for safe cleanup$/i,
    "无法隔离引擎进程，请重试。",
  ],
  [
    /^Choose a workspace before starting the agent$/i,
    "请先选择工作区，再开始任务。",
  ],
  [
    /^Configure your API group before starting the agent$/i,
    "请先选择分组和模型，再开始任务。",
  ],
  [
    /^The API key is unavailable\. Run configuration again\.$/i,
    "API 密钥不可用。请重新保存模型配置。",
  ],
  [
    /^Authentication expired or credentials were rejected \(HTTP 401\)$/i,
    "登录已过期或凭证被拒绝，请重新登录。",
  ],
  [
    /^Accept the account service agreement before signing in$/i,
    "请先同意账户服务协议后再登录。",
  ],
  [
    /^No supported API group is available for this account$/i,
    "当前账户没有可用分组，请在后台检查权限。",
  ],
  [/^Select a model$/i, "请选择一个模型。"],
  [/^Invalid model identifier$/i, "模型标识无效。"],
  [
    /^The selected group is not available for this account$/i,
    "当前账户无法使用所选分组。",
  ],
  [
    /^Stop the current response before changing configuration$/i,
    "请先停止当前任务，再更改配置。",
  ],
  [
    /^Too many agent requests are already running$/i,
    "同时进行的任务过多，请稍后再试。",
  ],
  [/^This conversation is already running$/i, "当前对话已在运行。"],
  [/^Invalid request ID$/i, "请求标识无效。"],
  [
    /^Prompt must contain 1 to 65536 UTF-8 bytes$/i,
    "任务内容长度需要在 1 到 65536 字节之间。",
  ],
  [/^Invalid OpenCode session ID$/i, "会话标识无效。"],
  [
    /^The selected API line failed TLS verification(?: \(.+\))?\. Switch to the backup line in Settings and retry\.$/i,
    "当前线路证书校验失败。请在设置中切换到备用线路后再试。",
  ],
  [/^Cannot write to the operating system credential vault$/i, "无法写入系统凭据库。"],
  [/^Cannot read the operating system credential vault$/i, "无法读取系统凭据库。"],
  [/^Cannot delete the operating system credential$/i, "无法删除系统凭据。"],
  [/^Credential vault task failed$/i, "系统凭据库操作失败。"],
  [/^Cannot initialize the API client$/i, "无法初始化接口客户端。"],
  [/^Invalid API endpoint index$/i, "线路编号无效。"],
  [/^Folder picker was interrupted$/i, "选择文件夹已中断。"],
  [/^A valid account and group are required$/i, "需要有效的账户和分组。"],
  [/^The pending API key credential is invalid$/i, "待写入的 API 密钥无效。"],
  [
    /^The created API key is bound to an unexpected group$/i,
    "新创建的 API 密钥绑定了错误的分组。",
  ],
  [
    /^The API key already exists but could not be verified for this account$/i,
    "API 密钥已存在，但无法核对此账户。",
  ],
  [/^Email and password are required$/i, "请输入邮箱和密码。"],
  [/^Enter the six-digit authentication code$/i, "请输入六位验证码。"],
  [
    /^Enter a valid six-digit authentication code$/i,
    "请输入有效的六位验证码。",
  ],
  [/^Login response is missing the user profile$/i, "登录响应缺少账户信息。"],
  [/^Choose a workspace before opening it$/i, "请先选择工作区，再打开文件夹。"],
  [/^Choose a workspace$/i, "请选择一个工作区。"],
  [/^Invalid workspace path$/i, "工作区路径无效。"],
  [
    /^Workspace must be an existing absolute directory$/i,
    "工作区必须是已存在的绝对路径目录。",
  ],
  [/^Cannot open workspace(?::\s*.+)?$/i, "无法打开工作区。"],
  [
    /^Cannot write OpenCode configuration(?::\s*.+)?$/i,
    "无法写入引擎配置。",
  ],
  [/^Cannot open settings(?::\s*.+)?$/i, "无法打开本地设置。"],
  [/^Invalid settings(?::\s*.+)?$/i, "本地设置无效。"],
  [/^Cannot save settings(?::\s*.+)?$/i, "无法保存本地设置。"],
  [/^Process cleanup timed out$/i, "引擎进程清理超时，请重试。"],
  [
    /^The application channel disconnected$/i,
    "与界面的连接已断开，请重试。",
  ],
  [
    /^The interface stopped acknowledging stream data$/i,
    "界面停止确认数据，任务已中断。",
  ],
  [
    /^The agent supervisor stopped unexpectedly$/i,
    "引擎监管意外停止，请重试。",
  ],
  [
    /^The agent process exceeded its cleanup deadline$/i,
    "引擎进程清理超时，请重试。",
  ],
  [
    /^OpenCode returned invalid or oversized stream output$/i,
    "本地引擎返回了无效或过大的输出。",
  ],
  [/^Both API endpoints are unavailable$/i, "主线路和备用线路都无法连接。"],
  [
    /^The API rejected the request; check account permissions and settings$/i,
    "接口拒绝了请求，请检查账户权限和设置。",
  ],
  [/^API response exceeded the size limit$/i, "接口响应超出大小限制。"],
  [/^Invalid API request path$/i, "接口路径无效。"],
  [/^Checking OpenCode$/i, "正在检查本地引擎"],
  [/^Starting OpenCode$/i, "正在启动本地引擎"],
  [/^OpenCode ready$/i, "本地引擎已就绪"],
];

function withOriginal(message: string, original: string): LocalizedError {
  return original === message ? { message } : { message, original };
}

export function localizeError(raw?: string | null): LocalizedError {
  const original = String(raw ?? "").trim();
  if (!original) return { message: "操作失败，请稍后重试。" };
  for (const [pattern, message] of TRANSLATIONS) {
    if (pattern.test(original)) return withOriginal(message, original);
  }
  if (/[\u4e00-\u9fff]/.test(original)) return { message: original };
  return withOriginal("操作失败，请稍后重试。", original);
}

export function errorRaw(error: unknown): string {
  if (error instanceof Error) return error.message.trim();
  if (typeof error === "string") return error.trim();
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message).trim();
  }
  return "";
}

export function errorDisplay(error: unknown): LocalizedError {
  const original = errorRaw(error);
  return original ? localizeError(original) : { message: "操作失败，请稍后重试。" };
}

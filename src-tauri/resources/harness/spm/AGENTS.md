# SPM 记忆

缺决策、约束或进度时先 recall，不要让用户复述已经保存过的事实。
只把 evidence_decision.status 为 supported 的结果当作已证实；insufficient / unknown 时继续手头工作或开口询问。
需要原文时调用 read，把 recall 返回的 read_token 原样放进数组一次传入。
耐久偏好和项目决策用 remember，并带稳定 idempotency_key；不要保存密钥、一次性日志或工具参数。
同一轮不要重复空 recall。当前文件、Git 和运行时观察优先于旧记忆。

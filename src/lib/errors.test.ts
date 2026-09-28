// @vitest-environment node
import { describe, expect, it } from "vitest";
import { errorDisplay, errorRaw, localizeError } from "./errors";

describe("localizeError", () => {
  it("translates known OpenCode login and session failures", () => {
    expect(localizeError("Sign in before configuring OpenCode")).toEqual({
      message: "请先登录后再配置模型。",
      original: "Sign in before configuring OpenCode",
    });
    expect(
      localizeError("Unable to create an OpenCode session: HTTP 500: boom"),
    ).toEqual({
      message: "无法创建本地会话。",
      original: "Unable to create an OpenCode session: HTTP 500: boom",
    });
  });

  it("keeps Chinese text and falls back for unknown English", () => {
    expect(localizeError("工作区尚未切换完成，请稍后重试。")).toEqual({
      message: "工作区尚未切换完成，请稍后重试。",
    });
    expect(localizeError("mysterious backend failure")).toEqual({
      message: "操作失败，请稍后重试。",
      original: "mysterious backend failure",
    });
  });

  it("extracts the original message before translation", () => {
    expect(errorRaw(new Error("Unable to create an OpenCode session"))).toBe(
      "Unable to create an OpenCode session",
    );
    expect(errorDisplay("Sign in before starting the agent")).toEqual({
      message: "请先登录后再开始任务。",
      original: "Sign in before starting the agent",
    });
    expect(localizeError("Agent is working")).toEqual({
      message: "正在生成",
      original: "Agent is working",
    });
    expect(localizeError("OpenCode reported an error")).toEqual({
      message: "本地引擎报告了一条错误。",
      original: "OpenCode reported an error",
    });
  });
});

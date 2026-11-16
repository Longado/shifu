import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";

export default function (pi) {
  const catalog = JSON.parse(readFileSync(new URL("./catalog.json", import.meta.url), "utf8"));
  pi.registerProvider("stepfun", {
    baseUrl: "https://api.stepfun.ai/step_plan/v1",
    apiKey: "STEP_API_KEY",
    api: "openai-completions",
    models: catalog.map(({ id, label }) => ({
      id, name: label, reasoning: false, input: ["text"],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 128000, maxTokens: 8192,
      compat: { supportsStore: false, supportsDeveloperRole: false, maxTokensField: "max_tokens" },
    })),
  });

  function hook(event, ctx, lastMessage) {
    if (!process.env.SHIFU_ID || !process.env.SHIFU_STEPFUN_BIN) return Promise.resolve();
    const input = JSON.stringify({
      session_id: ctx.sessionManager.getSessionFile(),
      ...(lastMessage ? { last_assistant_message: lastMessage } : {}),
    });
    return new Promise((resolve) => {
      const child = execFile(process.env.SHIFU_STEPFUN_BIN, ["hook", "stepfun", event],
        { timeout: 5000 }, (error) => {
          if (error) console.error("Shifu: StepFun 状态回传失败");
          resolve();
        });
      child.stdin.on("error", () => {}); // The hook may exit before it needs stdin when the agent was removed.
      child.stdin.end(input);
    });
  }
  pi.on("session_start", (_event, ctx) => hook("SessionStart", ctx));
  pi.on("agent_start", (_event, ctx) => hook("UserPromptSubmit", ctx));
  pi.on("agent_end", (event, ctx) => {
    const last = [...event.messages].reverse().find((m) => m.role === "assistant");
    const text = last?.content.filter((c) => c.type === "text").map((c) => c.text).join("\n");
    return hook("Stop", ctx, text);
  });
  pi.on("session_shutdown", (_event, ctx) => hook("SessionEnd", ctx));
}

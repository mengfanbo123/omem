import type { Plugin } from "@opencode-ai/plugin";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { OmemClient } from "./client.js";
import { autoRecallHook, compactingHook, keywordDetectionHook, sessionEndHook } from "./hooks.js";
import { getUserTag, getProjectTag } from "./tags.js";
import { buildTools } from "./tools.js";

const OmemPlugin: Plugin = async ({ directory, client }) => {
  // Config priority: opencode.json plugin_config > ~/.config/ourmem/config.json > env > default
  let apiUrl = "https://api.ourmem.ai";
  let apiKey = "";

  // Level 4: env vars (backward compat)
  if (process.env.OMEM_API_URL) apiUrl = process.env.OMEM_API_URL;
  if (process.env.OMEM_API_KEY) apiKey = process.env.OMEM_API_KEY;

  // Level 3: global config file
  try {
    const globalCfg = JSON.parse(readFileSync(join(homedir(), ".config", "ourmem", "config.json"), "utf-8"));
    if (globalCfg.apiUrl) apiUrl = globalCfg.apiUrl;
    if (globalCfg.apiKey) apiKey = globalCfg.apiKey;
  } catch {}

  // Level 2: opencode.json plugin_config (project-level, highest priority)
  try {
    const ocCfg = JSON.parse(readFileSync(join(directory, "opencode.json"), "utf-8"));
    const pc = ocCfg?.plugin_config?.["@ourmem/opencode"];
    if (pc?.apiUrl) apiUrl = pc.apiUrl;
    if (pc?.apiKey) apiKey = pc.apiKey;
  } catch {}

  const omemClient = new OmemClient(apiUrl, apiKey);

  const email = process.env.GIT_AUTHOR_EMAIL || process.env.USER || "unknown";
  const cwd = directory || process.cwd();
  const containerTags = [getUserTag(email), getProjectTag(cwd)];

  return {
    "experimental.chat.system.transform": autoRecallHook(omemClient, containerTags),
    "chat.message": keywordDetectionHook(),
    "experimental.session.compacting": compactingHook(omemClient, containerTags),
    tool: buildTools(omemClient, containerTags),
    "shell.env": async (_input, output) => {
      if (directory) {
        output.env.OMEM_PROJECT_DIR = directory;
      }
    },
    event: sessionEndHook(client, omemClient, containerTags),
  };
};

export { OmemPlugin };

export default {
  id: "ourmem",
  server: OmemPlugin,
};

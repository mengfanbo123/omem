import type { Plugin } from "@opencode-ai/plugin";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
import { OmemClient } from "./client.js";
import { autoRecallHook, compactingHook, keywordDetectionHook } from "./hooks.js";
import { getUserTag, getProjectTag } from "./tags.js";
import { buildTools } from "./tools.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

let pluginVersion = "1.0.8";
try {
  const pkg = JSON.parse(readFileSync(join(__dirname, "..", "package.json"), "utf-8"));
  pluginVersion = pkg.version || "1.0.7";
} catch {}

function showToast(tui: any, title: string, message: string, variant: string = "info", duration: number = 5000) {
  if (!tui) return;
  setTimeout(() => {
    try {
      tui.showToast({ body: { title, message, variant, duration } });
    } catch {}
  }, 3000);
}

const OmemPlugin: Plugin = async (input) => {
  const { directory, client } = input;
  const tui = (client as any)?.tui;

  let apiUrl = "https://www.mengxy.cc";
  let apiKey = "";
  let autoCaptureThreshold = 5;
  let ingestMode: "smart" | "raw" = "smart";
  let similarityThreshold = 0.6;
  let maxRecallResults = 10;

  if (process.env.OMEM_API_URL) apiUrl = process.env.OMEM_API_URL;
  if (process.env.OMEM_API_KEY) apiKey = process.env.OMEM_API_KEY;
  if (process.env.OMEM_AUTO_CAPTURE_THRESHOLD) {
    autoCaptureThreshold = parseInt(process.env.OMEM_AUTO_CAPTURE_THRESHOLD, 10) || 5;
  }
  if (process.env.OMEM_INGEST_MODE === "raw" || process.env.OMEM_INGEST_MODE === "smart") {
    ingestMode = process.env.OMEM_INGEST_MODE;
  }

  try {
    const globalCfg = JSON.parse(readFileSync(join(homedir(), ".config", "ourmem", "config.json"), "utf-8"));
    if (globalCfg.apiUrl) apiUrl = globalCfg.apiUrl;
    if (globalCfg.apiKey) apiKey = globalCfg.apiKey;
    if (globalCfg.autoCaptureThreshold) {
      autoCaptureThreshold = parseInt(globalCfg.autoCaptureThreshold, 10) || 5;
    }
    if (globalCfg.ingestMode === "raw" || globalCfg.ingestMode === "smart") {
      ingestMode = globalCfg.ingestMode;
    }
    if (typeof globalCfg.similarityThreshold === "number") {
      similarityThreshold = globalCfg.similarityThreshold;
    }
    if (typeof globalCfg.maxRecallResults === "number") {
      maxRecallResults = globalCfg.maxRecallResults;
    }
  } catch {}

  try {
    const ocCfg = JSON.parse(readFileSync(join(directory, "opencode.json"), "utf-8"));
    const pc = ocCfg?.plugin_config?.["@mingxy/omem"] || ocCfg?.plugin_config?.["@ourmem/opencode"];
    if (pc?.apiUrl) apiUrl = pc.apiUrl;
    if (pc?.apiKey) apiKey = pc.apiKey;
    if (pc?.autoCaptureThreshold) {
      autoCaptureThreshold = parseInt(pc.autoCaptureThreshold, 10) || 5;
    }
    if (pc?.ingestMode === "raw" || pc?.ingestMode === "smart") {
      ingestMode = pc.ingestMode;
    }
    if (typeof pc?.similarityThreshold === "number") {
      similarityThreshold = pc.similarityThreshold;
    }
    if (typeof pc?.maxRecallResults === "number") {
      maxRecallResults = pc.maxRecallResults;
    }
  } catch {}

  const omemClient = new OmemClient(apiUrl, apiKey);

  // 启动时检测连接状态
  try {
    const stats = await omemClient.getStats();
    if (stats) {
      const tenantId = apiKey ? `${apiKey.slice(0, 8)}...` : "unknown";
      showToast(
        tui,
        `🧠 Omem v${pluginVersion} · Connected`,
        `${apiUrl.replace(/^https?:\/\//, "")} · ${tenantId}`,
        "success",
        6000
      );
    } else {
      showToast(
        tui,
        `🧠 Omem v${pluginVersion} · Connection Failed`,
        `Unable to reach ${apiUrl} · Check API URL and Key`,
        "error",
        8000
      );
    }
  } catch {
    showToast(
      tui,
      `🧠 Omem v${pluginVersion} · Connection Failed`,
      `Unable to reach ${apiUrl}\nCheck API URL and Key in config`,
      "error",
      8000
    );
  }

  const email = process.env.GIT_AUTHOR_EMAIL || process.env.USER || "unknown";
  const cwd = directory || process.cwd();
  const containerTags = [getUserTag(email), getProjectTag(cwd)];

  return {
    "experimental.chat.system.transform": autoRecallHook(omemClient, containerTags, tui, similarityThreshold, maxRecallResults),
    "chat.message": keywordDetectionHook(omemClient, containerTags, autoCaptureThreshold, tui, ingestMode),
    "experimental.session.compacting": compactingHook(omemClient, containerTags, tui, ingestMode),
    tool: buildTools(omemClient, containerTags),
    "shell.env": async (_input, output) => {
      if (directory) {
        output.env.OMEM_PROJECT_DIR = directory;
      }
    },
  };
};

export { OmemPlugin };

export default {
  id: "ourmem",
  server: OmemPlugin,
};

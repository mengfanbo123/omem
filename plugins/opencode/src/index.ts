import type { Plugin } from "@opencode-ai/plugin";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { OmemClient } from "./client.js";
import { autoRecallHook, compactingHook, keywordDetectionHook } from "./hooks.js";
import { getUserTag, getProjectTag } from "./tags.js";
import { buildTools } from "./tools.js";

const OmemPlugin: Plugin = async (input) => {
  const { directory, client } = input;
  const tui = (client as any)?.tui;

  let apiUrl = "https://api.ourmem.ai";
  let apiKey = "";
  let autoCaptureThreshold = 5;
  let ingestMode: "smart" | "raw" = "smart";
  let similarityThreshold = 0.6;

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
  } catch {}

  const omemClient = new OmemClient(apiUrl, apiKey);

  const email = process.env.GIT_AUTHOR_EMAIL || process.env.USER || "unknown";
  const cwd = directory || process.cwd();
  const containerTags = [getUserTag(email), getProjectTag(cwd)];

  return {
    "experimental.chat.system.transform": autoRecallHook(omemClient, containerTags, tui, similarityThreshold),
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

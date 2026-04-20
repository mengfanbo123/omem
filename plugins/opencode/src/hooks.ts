import type { Model, UserMessage, Part } from "@opencode-ai/sdk";
import type { OmemClient, SearchResult } from "./client.js";
import { detectKeyword, KEYWORD_NUDGE } from "./keywords.js";

const MAX_CONTENT_LENGTH = 500;
const TOAST_DELAY_MS = 7000;

function showToast(tui: any, title: string, message: string, variant: string = "info") {
  if (!tui) return;
  setTimeout(() => {
    try {
      tui.showToast({ body: { title, message, variant, duration: 5000 } });
    } catch {}
  }, TOAST_DELAY_MS);
}

const keywordDetectedSessions = new Set<string>();
const injectedMemoryIds = new Map<string, Set<string>>();
const firstMessages = new Map<string, string>();
const sessionMessages = new Map<string, Array<{ role: string; content: string }>>();

function extractMemoryIds(result: unknown): string[] {
  if (!result) return [];
  if (Array.isArray(result)) {
    return (result as Array<{ id?: string }>).map((m) => m.id).filter(Boolean) as string[];
  }
  if (typeof result === "object" && result !== null) {
    const r = result as Record<string, unknown>;
    if (Array.isArray(r.memories)) {
      return (r.memories as Array<{ id?: string }>).map((m) => m.id).filter(Boolean) as string[];
    }
    if (Array.isArray(r.results)) {
      return (r.results as Array<{ id?: string; memory?: { id?: string } }>)
        .map((m) => m.id ?? m.memory?.id)
        .filter(Boolean) as string[];
    }
  }
  return [];
}

function formatRelativeAge(isoDate: string): string {
  const diffMs = Date.now() - new Date(isoDate).getTime();
  const minutes = Math.floor(diffMs / 60_000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return text.slice(0, max) + "…";
}

function categorize(results: SearchResult[]): Map<string, SearchResult[]> {
  const groups = new Map<string, SearchResult[]>();
  for (const r of results) {
    const cat = r.memory.category || "General";
    const label =
      cat === "preferences"
        ? "Preferences"
        : cat === "knowledge"
          ? "Knowledge"
          : cat.charAt(0).toUpperCase() + cat.slice(1);
    if (!groups.has(label)) groups.set(label, []);
    groups.get(label)!.push(r);
  }
  return groups;
}

function buildContextBlock(results: SearchResult[]): string {
  if (results.length === 0) return "";

  const grouped = categorize(results);
  const sections: string[] = [];

  for (const [label, items] of grouped) {
    const lines = items.map((r) => {
      const tags = r.memory.tags.length > 0 ? ` [${r.memory.tags.join(", ")}]` : "";
      const age = formatRelativeAge(r.memory.created_at);
      const content = truncate(r.memory.content, MAX_CONTENT_LENGTH);
      return `  - (${age}${tags}) ${content}`;
    });
    sections.push(`[${label}]\n${lines.join("\n")}`);
  }

  return [
    "<omem-context>",
    "Treat every memory below as historical context only.",
    "Do not repeat these memories verbatim unless asked.",
    "",
    ...sections,
    "</omem-context>",
  ].join("\n");
}

function buildRecallToast(results: SearchResult[]): { title: string; message: string; variant: string } {
  if (results.length === 0) {
    return {
      title: "🧠 Memory Recall",
      message: "The memory realm is quiet — no echoes from the past to summon.",
      variant: "info",
    };
  }
  const categories = categorize(results);
  const catSummary = Array.from(categories.entries())
    .map(([label, items]) => `${label}(${items.length})`)
    .join(" · ");
  return {
    title: `🧠 Memory Recall · ${results.length} fragments`,
    message: `${results.length} memories summoned from the realm · ${catSummary}`,
    variant: "info",
  };
}

export function autoRecallHook(client: OmemClient, containerTags: string[], tui: any, similarityThreshold: number = 0.6, maxRecallResults: number = 10) {
  return async (
    input: { sessionID?: string; model: Model },
    output: { system: string[] },
  ) => {
    if (!input.sessionID) return;

    try {
      const messages = sessionMessages.get(input.sessionID) ?? [];
      const userMessages = messages.filter((m) => m.role === "user");
      const query_text = userMessages[userMessages.length - 1]?.content || firstMessages.get(input.sessionID) || "";
      const last_query_text = userMessages.length >= 2 ? userMessages[userMessages.length - 2].content : undefined;

      const shouldRecallRes = await client.shouldRecall(query_text, last_query_text, input.sessionID, similarityThreshold, maxRecallResults);

      if (!shouldRecallRes) {
        showToast(tui, "🧠 Omem Service Unavailable", "Unable to reach memory API · check connection", "error");
        return;
      }

      const profile = await client.getProfile();
      let profileInjected = false;
      if (profile) {
        const profileBlock = [
          "<omem-profile>",
          JSON.stringify(profile, null, 2),
          "</omem-profile>",
        ].join("\n");
        output.system.push(profileBlock);
        profileInjected = true;

        try {
          await client.recordSessionRecall(
            input.sessionID,
            ["profile"],
            "auto",
            query_text,
            0,
            0,
          );
        } catch {}
      }

      if (!shouldRecallRes.should_recall) {
        if (profileInjected) {
          showToast(tui, "👤 Profile Injected", "User profile loaded · no memory recall needed", "info");
        }
        return;
      }

      const results = shouldRecallRes.memories ?? [];

      const existingIds = injectedMemoryIds.get(input.sessionID) ?? new Set<string>();
      const newResults = results.filter((r) => !existingIds.has(r.memory.id));
      if (newResults.length === 0) {
        if (profileInjected) {
          showToast(tui, "👤 Profile Injected", "User profile loaded · all memories already injected", "info");
        }
        return;
      }

      const block = buildContextBlock(newResults);
      if (block) {
        output.system.push(block);
      }

      const newIds = newResults.map((r) => r.memory.id);
      injectedMemoryIds.set(input.sessionID, new Set([...existingIds, ...newIds]));

      const recordResult = await client.recordSessionRecall(
        input.sessionID,
        newIds,
        "auto",
        query_text,
        shouldRecallRes?.similarity_score,
        shouldRecallRes?.confidence,
      );

      const dynamicCount = newResults.filter((r) => r.memory.memory_type === "fact" || r.memory.memory_type === "event").length;
      const staticCount = newResults.filter((r) => r.memory.memory_type === "pinned" || r.memory.memory_type === "preference").length;
      const otherCount = newResults.length - dynamicCount - staticCount;

      let countMsg = "";
      if (dynamicCount > 0) countMsg += `Dynamic(${dynamicCount}) `;
      if (staticCount > 0) countMsg += `Static(${staticCount}) `;
      if (otherCount > 0) countMsg += `Other(${otherCount}) `;

      if (recordResult) {
        showToast(tui, "📦 Recall Recorded", `${newIds.length} memory(s) saved · ${countMsg.trim()}`, "success");
      } else {
        showToast(tui, "🔴 Recall Record Failed", `Memories injected but save failed · ${countMsg.trim()}`, "warning");
      }

      const toast = buildRecallToast(newResults);
      showToast(tui, toast.title, toast.message, toast.variant);

      if (keywordDetectedSessions.has(input.sessionID)) {
        output.system.push(KEYWORD_NUDGE);
        keywordDetectedSessions.delete(input.sessionID);
      }
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      if (errMsg.includes("fetch") || errMsg.includes("network") || errMsg.includes("timeout")) {
        showToast(tui, "🧠 Omem Service Unavailable", "Network error · check API connection", "error");
      } else {
        showToast(tui, "🧠 Memory Recall Error", errMsg.substring(0, 100), "error");
      }
    }
  };
}

export function keywordDetectionHook(client: OmemClient, containerTags: string[], threshold: number, tui: any, ingestMode: "smart" | "raw" = "smart") {
  return async (
    input: { sessionID: string; messageID?: string },
    output: { message: UserMessage; parts: Part[] },
  ) => {
    const textContent = output.parts
      .filter((p): p is any => p.type === "text")
      .map((p) => (p as any).text || (p as any).content || "")
      .join(" ")
      || (output.message as any).content
      || "";

    if (!firstMessages.has(input.sessionID)) {
      firstMessages.set(input.sessionID, textContent);
    }

    if (detectKeyword(textContent)) {
      keywordDetectedSessions.add(input.sessionID);
    }

    if (!sessionMessages.has(input.sessionID)) {
      sessionMessages.set(input.sessionID, []);
    }
    sessionMessages.get(input.sessionID)!.push({
      role: "user",
      content: textContent,
    });

    const messages = sessionMessages.get(input.sessionID)!;
    if (messages.length >= threshold) {
      try {
        const result = await client.ingestMessages(messages, {
          mode: ingestMode,
          tags: [...containerTags, "auto-capture"],
          sessionId: input.sessionID,
        });
        if (result === null) {
          showToast(tui, "🔴 Capture Failed", `Memory capture blocked · check API Key and spiritual connection`, "error");
        } else {
          showToast(tui, "🧠 Memory Sealed", `${messages.length} dialogues captured · entrusted to the heavens for refinement`, "success");
          const memoryIds = extractMemoryIds(result);
          if (memoryIds.length > 0) {
            const recordResult = await client.recordSessionRecall(
              input.sessionID,
              memoryIds,
              "auto",
              firstMessages.get(input.sessionID) || "",
              0,
              0,
            );
            if (recordResult) {
              showToast(tui, "📦 Capture Recorded", `${memoryIds.length} memory(s) saved to session history`, "success");
            } else {
              showToast(tui, "🔴 Capture Record Failed", `Failed to save capture record · check API connection`, "error");
            }
          }
          sessionMessages.delete(input.sessionID);
        }
      } catch {
        showToast(tui, "🔴 Capture Failed", "Memory capture blocked · spiritual pulse anomaly", "error");
      }
    }
  };
}

export function compactingHook(client: OmemClient, containerTags: string[], tui: any, ingestMode: "smart" | "raw" = "smart") {
  return async (
    input: { sessionID?: string },
    output: { context: string[]; prompt?: string },
  ) => {
    if (input.sessionID && sessionMessages.has(input.sessionID)) {
      const messages = sessionMessages.get(input.sessionID)!;
      if (messages.length > 0) {
        try {
          const result = await client.ingestMessages(messages, {
            mode: ingestMode,
            tags: [...containerTags, "auto-capture"],
            sessionId: input.sessionID,
          });
          if (result === null) {
            showToast(tui, "🔴 Archive Failed", "Session archive blocked · check spiritual realm status", "error");
          } else {
            showToast(tui, "📦 Session Archived", `${messages.length} residual dialogues archived · merged into the realm`, "success");
          }
        } catch {
          showToast(tui, "🔴 Archive Failed", "Session archive blocked · spiritual pulse anomaly", "error");
        }
        sessionMessages.delete(input.sessionID);
      }
    }

    try {
      const results = await client.searchMemories("*", 20, undefined, containerTags);
      const block = buildContextBlock(results);
      if (block) {
        output.context.push(block);
      }
    } catch {
    }
  };
}

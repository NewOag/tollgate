// Streamed responses are stored as the raw SSE bytes actually relayed to
// the client (one `data: {...}` frame per chunk — see
// rustgate/src/format/{openai,anthropic}.rs for the wire shapes). This
// walks those frames and reassembles the delta text into one string, so
// the "formatted" view can show the completion instead of dozens of
// individual JSON chunks.

function extractDeltaText(obj: any, format: string): string | null {
  if (format === "anthropic") {
    if (obj?.type === "content_block_delta" && obj.delta?.type === "text_delta" && typeof obj.delta.text === "string") {
      return obj.delta.text;
    }
    return null;
  }
  if (format === "openai_responses") {
    if (obj?.type === "response.output_text.delta" && typeof obj.delta === "string") {
      return obj.delta;
    }
    return null;
  }
  // OpenAI-style (and anything else that follows the same `choices[].delta`
  // shape, since most OpenAI-compatible providers do).
  const delta = obj?.choices?.[0]?.delta;
  if (delta && typeof delta.content === "string") return delta.content;
  return null;
}

/** Returns the assembled completion text from an SSE body, or `null` if
 * the body isn't SSE or no delta text was found (caller should fall back
 * to showing the raw body in that case). */
export function extractStreamText(raw: string, format: string): string | null {
  if (!raw || !raw.includes("data:")) return null;

  let text = "";
  let sawDelta = false;
  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed.startsWith("data:")) continue;
    const payload = trimmed.slice(5).trim();
    if (!payload || payload === "[DONE]") continue;

    let obj: unknown;
    try {
      obj = JSON.parse(payload);
    } catch {
      continue;
    }

    const delta = extractDeltaText(obj, format);
    if (delta !== null) {
      text += delta;
      sawDelta = true;
    }
  }

  return sawDelta ? text : null;
}

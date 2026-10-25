// Reading a request body with a hard limit. Kept out of index.mjs: the Workers runtime takes every
// named export of the entry module for a handler, so the entry exports nothing but its default.

// /mcp is public and unauthenticated; a real request is a few hundred bytes.
export const MAX_BODY = 64 * 1024;

/** The body as text, or null once it passes maxBytes (declared or counted); stops reading at the limit. */
export async function readCapped(request, maxBytes) {
  if (Number(request.headers.get("content-length")) > maxBytes) return null;
  if (!request.body) return "";
  const reader = request.body.getReader();
  const parts = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > maxBytes) {
      await reader.cancel();
      return null;
    }
    parts.push(value);
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const part of parts) {
    bytes.set(part, offset);
    offset += part.byteLength;
  }
  return new TextDecoder().decode(bytes);
}

# Telegram Attachments and Web Preview Design

Date: 2026-07-01

## Goal

Support files uploaded through Telegram, make them visible in the Web Console chat history, and pass them to the active AI backend when a turn runs.

The first release should support small previews plus download links in Web Console:

- Images show a thumbnail preview.
- PDF, text, CSV, JSON, and unknown files show a compact file card with name, size, type, and download action.
- All supported attachment types can be downloaded from Web Console.
- Attachments are available to opencode through local file paths when the backend supports file input.

## Current State

Wukong currently treats chat input as text-only across all transport layers.

- `wukong-tg-client::parse_updates` extracts only `message.text` and skips photos, documents, and other non-text messages.
- `wukong-telegram::handle_message` receives a `TgMessage` with only `text`, then passes a string to `run_turn`.
- `wukong-web` sends chat turns with `GET /chat?q=...` over `EventSource`, so there is no request body or multipart upload path.
- `wukong-gateway::AgentRequest` contains `prompt`, `session_id`, `thinking`, and `model`, but no attachments.
- The opencode CLI supports `--file`, but Wukong does not expose or use it.
- The opencode server integration currently sends only text message parts.

## Storage Model

Attachments should be stored as files under the configured Wukong workspace, with metadata stored in the chat history database.

Use Wukong chat identity, not opencode session identity, as the primary relationship:

- `scope` identifies the source conversation, for example `user:tg-123456`.
- `thread_id` identifies the chat thread in the history database.
- `message_id` identifies the user message that introduced the attachment.

Do not store files under opencode session ids. Opencode sessions are backend execution details; Web Console, Telegram, and chat history already share `scope`, `thread_id`, and `message_id`.

Recommended file layout:

```text
<WUKONG_WORKSPACE>/.wukong/uploads/<safe_scope>/<message_id>/<safe_filename>
```

Examples:

```text
/home/wukong/workspace/.wukong/uploads/user_tg-123456/42/report.pdf
/home/wukong/workspace/.wukong/uploads/user_tg-123456/43/photo.jpg
```

`safe_scope` and `safe_filename` must be sanitized before writing to disk. The database stores the original filename separately for display.

## Database Model

Add an attachment table to the chat history database.

Conceptual schema:

```text
chat_attachments
  id INTEGER PRIMARY KEY
  message_id INTEGER NOT NULL
  scope TEXT NOT NULL
  source TEXT NOT NULL
  original_name TEXT NOT NULL
  stored_name TEXT NOT NULL
  relative_path TEXT NOT NULL
  mime_type TEXT
  size_bytes INTEGER NOT NULL
  sha256 TEXT
  telegram_file_id TEXT
  created_at INTEGER NOT NULL
```

Rules:

- `message_id` links attachments to the user message shown in Web Console.
- `relative_path` is relative to the workspace upload root, not an arbitrary absolute path.
- Download handlers resolve `relative_path` under the upload root and reject path traversal.
- `source` starts with `telegram`; future values may include `web`.
- `telegram_file_id` is stored for traceability and possible re-download, but local files are the source of truth after ingestion.

## Telegram Data Flow

Telegram parsing should produce a richer message model.

```text
TgMessage
  update_id
  chat_id
  text
  attachments[]
```

Each Telegram attachment should include enough data to download and display the file:

```text
TgAttachment
  kind
  file_id
  unique_file_id
  original_name
  mime_type
  size_bytes
```

Parsing behavior:

- Text messages keep existing behavior.
- `document` messages become file attachments.
- `photo` messages choose the largest available photo size as the image attachment.
- `caption` becomes the prompt text when present.
- If a file has no caption, generate a fallback prompt such as `使用者上傳了 report.pdf，請分析附件內容。`
- Non-text, non-file update types continue to advance the Telegram offset and are not re-delivered forever.

Ingestion behavior:

1. Accept the Telegram update if the chat is allowed.
2. Create the user chat message with caption/text/fallback prompt.
3. Download Telegram files with `getFile` and the file download endpoint.
4. Store files under `.wukong/uploads/<safe_scope>/<message_id>/`.
5. Insert attachment metadata rows for the message.
6. Run the turn with the prompt and the stored attachment paths.
7. Record assistant output and live events as today.

If a file download fails, record the user message and an error live event, then reply with a clear Telegram error. Do not run the model with missing attachments unless the message also has normal text and the error explicitly says attachments were skipped.

## Web Console Data Flow

Chat history APIs should include attachments with messages.

`GET /api/chat/messages` should return each message with an `attachments` array:

```json
{
  "id": 42,
  "role": "user",
  "content": "請分析這份 PDF",
  "attachments": [
    {
      "id": 7,
      "original_name": "report.pdf",
      "mime_type": "application/pdf",
      "size_bytes": 123456,
      "download_url": "/api/chat/attachments/7",
      "preview_url": null
    }
  ]
}
```

Add download and preview endpoints:

```text
GET /api/chat/attachments/:id
GET /api/chat/attachments/:id/preview
```

Endpoint behavior:

- Both endpoints require the same token protection as the chat APIs.
- Attachment lookup must verify the selected scope when a scope parameter is provided.
- Download streams the original file with a safe `Content-Disposition` filename.
- Preview is enabled for images only in the first release.
- Image preview can initially stream the original image with browser sizing; generated thumbnails can be added later if large images become a problem.
- PDF and other documents do not need inline rendering in the first release. They use file cards and download links.

Web UI behavior:

- Render attachments under the owning message bubble.
- Image attachments show a small thumbnail and filename.
- PDF, text, CSV, JSON, and unknown attachments show a compact file card.
- Each attachment has a download action.
- Loading older messages and switching scopes must preserve attachment rendering.
- Live Telegram events can initially show attachments after the message list reloads; real-time attachment cards can be added later if needed.

## Backend Data Flow

Extend the gateway request model with attachments.

```text
AgentRequest
  prompt
  session_id
  thinking
  model
  attachments[]

AgentAttachment
  path
  original_name
  mime_type
```

CLI backend behavior:

- Append `--file <path>` for every attachment before the prompt argument.
- Keep existing behavior unchanged when there are no attachments.
- Use local stored paths from the upload root.

Opencode server backend behavior:

- First verify whether the server API supports file message parts.
- If the server API supports file parts, map `AgentAttachment` to that API.
- If the server API does not support file parts, return a clear unsupported-backend error for attachment turns instead of silently dropping files.

The first implementation may support attachments only on the CLI backend if server API support is uncertain. Dropping attachments silently is not acceptable.

## Security and Limits

Apply conservative limits at ingestion time:

- Sanitize all filenames and never trust Telegram-provided names as paths.
- Store only under the workspace upload root.
- Enforce a maximum file size before download when Telegram provides file size.
- Enforce a maximum actual downloaded size during streaming.
- Restrict preview responses to known safe image MIME types.
- Do not execute uploaded files.
- Use token authorization for all Web attachment endpoints.

Recommended first-release default limits:

- Per-file max: 25 MiB.
- Per-message max attachments: 5.
- Image preview max inline size: 10 MiB; larger images still download but do not preview inline.

These limits should be constants or settings with clear error messages. They do not need a Web settings UI in the first release.

## Error Handling

Telegram user-facing errors:

- File too large: `⚠️ 檔案超過目前支援大小，請改用較小的檔案。`
- Download failed: `⚠️ 無法下載 Telegram 檔案，請稍後再試。`
- Backend does not support attachments: `⚠️ 目前的 agent backend 不支援附件輸入。`

Web Console errors:

- Missing file: show the attachment card with a disabled download state and `檔案不存在`.
- Unauthorized: return `401` as existing APIs do.
- Unsupported preview: return `404` or omit `preview_url`; the UI should still show download.

Backend errors:

- Never silently drop attachments.
- If attachment paths cannot be resolved, fail the turn before calling the backend.
- If server mode does not support file parts, return a specific error message.

## Testing

Unit tests:

- Telegram parser extracts text, document, photo, and caption messages.
- Telegram parser still advances offsets for skipped update types.
- Filename and scope sanitization reject path traversal.
- Chat history store inserts and lists attachments for a message.
- Gateway CLI argv includes `--file <path>` for each attachment.
- Gateway CLI argv remains unchanged for text-only requests.
- Server backend errors clearly when attachments are present but unsupported.

Web tests:

- `GET /api/chat/messages` includes attachments.
- Attachment download requires token when configured.
- Attachment download rejects wrong scope or path traversal.
- Image preview returns image content for image attachments.
- Non-image attachments omit preview or return unsupported.

Integration-style tests:

- Telegram document update creates a user message, stores a file, stores metadata, and calls backend with an attachment path.
- Web Console message history shows the Telegram-originated attachment metadata.

Manual verification:

- Send an image to Telegram and confirm Web Console shows a thumbnail and download link.
- Send a PDF to Telegram and confirm Web Console shows a file card and download link.
- Confirm the model receives the attachment when using the CLI backend.
- Confirm server backend reports a clear unsupported error if file parts are not available.

## Non-Goals for First Release

- Web-originated uploads.
- Inline PDF rendering.
- OCR or text extraction before sending to the model.
- Persistent thumbnail generation.
- Attachment cleanup UI.
- Cross-user sharing of attachment URLs.

## Open Decisions Resolved

- Attachment storage is based on Wukong `scope` and `message_id`, not opencode session id.
- Web Console should show small previews when safe and always offer download.
- The first release should prefer a simple file-card UI over rich document viewers.
- Attachments must not be silently ignored by any backend.

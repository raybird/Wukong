# Opencode-Inspired Web Console Chat Design

Date: 2026-07-04

## Goal

Evolve Wukong Web Console's chat page by borrowing interaction patterns from opencode web while keeping Wukong's own backend, chat history, Telegram integration, memory, skills, schedules, and settings intact.

This is a UI and interaction redesign, not an opencode web embed.

## Decision

Use opencode web as a UX reference only.

- Keep the current Wukong Axum API and browser-side custom element architecture.
- Keep Wukong as the owner of chat history, scope selection, unread state, question replies, and activity persistence.
- Do not iframe, proxy, or bundle opencode web assets.
- Do not let the browser talk directly to `opencode serve`.
- Do not replace Wukong chat history with opencode session state.

## Current Context

Wukong Web Console is a plain custom-elements SPA served by `wukong-web`.

- `static/index.html` defines the shell and nav.
- `static/app.js` routes hashes to custom elements.
- `wukong-chat.js` owns chat loading, rendering, live stream handling, question cards, attachments, and unread marker behavior.
- `wukong-web/src/lib.rs` exposes APIs for chat scopes, messages, message steps/events, attachments, questions, settings, memory, skills, schedules, and system diagnostics.
- The gateway already supports `opencode serve` for backend execution, including sessions, async prompts, event stream mapping, and question reply/reject.

The main gap is not backend connectivity. The gap is that the Web Console chat UI is still a single long chat component instead of a session-oriented workbench with inspectable message parts and activity.

## Product Shape

The redesigned chat page should feel like a Wukong workbench: a focused place to continue a conversation, inspect what the agent did, and answer pending questions without losing context.

It should borrow these opencode-style ideas:

- A clear thread/session frame rather than a loose chat log.
- Message rows made of parts, not only bubbles.
- Tool, reasoning, step, attachment, and question activity as first-class cards.
- Keyboard-friendly composer and navigation.
- Explicit session/source state so users know which scope they are operating in.

It should remain visibly Wukong:

- Traditional Chinese UI copy.
- Existing gold/dark palette direction may evolve but should not become a clone of opencode.
- Memory, skills, Telegram scope, schedules, and system status stay part of the Wukong Console identity.

## Proposed Architecture

Split the current chat component into focused browser modules while preserving the existing API surface.

```text
wukong-chat.js
  owns lifecycle, API calls, scope switching, live stream, composer orchestration

chat-message.js
  renders user/assistant/status message frames and message metadata

chat-activity.js
  renders reasoning, tool use, steps, events, and lazy activity details

chat-question-card.js
  renders OpenCode question prompts and reply/reject interactions

chat-thread-header.js
  renders selected scope/session context, model, skills, unread status, and jump controls

unread-marker.mjs
  remains the pure localStorage helper for per-scope last-seen message ids
```

`wukong-chat.js` should become the coordinator rather than the renderer for every DOM shape.

The first implementation can introduce these modules incrementally. It does not need to split everything at once if doing so would make review harder, but the final design target is clear component boundaries.

## UI Layout

Use a two-zone chat workbench that works on desktop and mobile.

Desktop:

```text
┌─────────────────────────────────────────────────────────────┐
│ Wukong nav                                                   │
├─────────────────────────────────────────────────────────────┤
│ Thread header: scope, model, skills, jump, status            │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│ Conversation rail                                            │
│  ├─ date marker                                              │
│  ├─ user message frame                                       │
│  ├─ assistant frame                                          │
│  │   ├─ activity cards: thinking / tools / steps             │
│  │   └─ answer part                                          │
│  ├─ unread divider                                           │
│  └─ pending question card                                    │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│ Composer: multiline input, send, command affordances         │
└─────────────────────────────────────────────────────────────┘
```

Mobile:

- Keep one vertical column.
- Collapse secondary thread status into a compact header row.
- Preserve large tap targets for scope, jump, question options, and composer send.
- Activity cards should default collapsed when space is tight, except active reasoning/question state.

## Visual Direction

Subject: Wukong as a personal agent workbench for Taiwanese developers and power users who want an inspectable assistant rather than a black-box chat bot.

Palette tokens:

- `Ink Black #11100E`: primary background.
- `Temple Bronze #B9852A`: Wukong accent for active states and unread divider.
- `Parchment #ECE3CF`: high-contrast readable text on dark surfaces.
- `Bamboo Mist #90A98B`: calm success/idle state.
- `Cinnabar #D05A3A`: error and attention state.
- `Slate Smoke #262827`: panels, card boundaries, and inactive surfaces.

Type direction:

- Display and headings: keep system-safe but use heavier weight, tighter tracking, and restrained gold accents rather than importing remote fonts.
- Body: system sans for reliable CJK rendering.
- Utility/data labels: tabular numeric system font where status counts, ids, and timestamps are shown.

Signature element:

- A vertical "activity rail" inside assistant message frames. Each reasoning/tool/step/question marker attaches to the assistant answer like workbench annotations on a scroll. This is the distinctive Wukong interpretation of opencode message parts.

Self-critique of direction:

- This avoids generic SaaS cards and avoids copying opencode's whole shell.
- The main aesthetic risk is the activity rail: if overdone it can feel busy. The implementation should keep it quiet by default and only emphasize active or unread work.

## Data Flow

No backend data shape changes are required for the first pass.

Existing APIs remain the source of truth:

- `GET /api/chat/scopes` for scope/thread choices.
- `GET /api/chat/messages` for paginated chat history.
- `GET /api/chat/stream` for Telegram-scope live events.
- `GET /api/chat/messages/:id/events` for lazy reasoning/tool history.
- `GET /api/chat/messages/:id/steps` for lazy helper outputs.
- `POST /api/questions/:request_id/reply` for answers.
- `POST /api/questions/:request_id/reject` for cancellation.
- `/chat` EventSource remains the direct send path for Web Console prompts.

Frontend rendering should normalize each message into display sections:

```text
message frame
  metadata: role, timestamp, status, ids when useful
  activity: lazy events, lazy steps, live progress, question card
  content: final user or assistant text/html
  attachments: optional attachment cards
```

Unread marker behavior stays per scope and should survive the layout redesign.

## Interaction Behavior

Thread header:

- Shows selected scope label, model status, skill preference status, and jump-to-date controls.
- Scope switching records latest seen state when appropriate, then reloads the selected thread.

Conversation:

- Latest load restores unread position when a marker exists.
- Older pagination preserves scroll position.
- Activity cards can be expanded without causing a forced scroll to bottom.
- Live progress scrolls only when the user is near the bottom.

Activity cards:

- Reasoning and tool history appear above the assistant answer, matching current live-turn ordering.
- Helper steps remain collapsible and visually secondary.
- Pending questions should be visually prominent and keyboard accessible.

Composer:

- Preserve Enter to send and Shift+Enter for newline.
- Keep auto-resize behavior.
- Clear unread divider and record latest seen state before sending a new prompt.

## Error Handling

- API failures should render clear inline states in the area affected, not replace the entire app shell.
- Lazy activity load failures should stay retryable by reopening the card.
- Question reply/reject failures should keep the card active and show the exact failure state.
- If localStorage is unavailable, unread behavior should degrade silently and the chat should still load.

## Testing Strategy

Automated checks should cover both pure helpers and served static assets.

- Keep `unread-marker.test.mjs` for pure localStorage/read marker behavior.
- Add or update Rust web tests that inspect static asset routes when new modules are served.
- Extend existing web component string tests where useful for expected class names such as activity rail, question card, and unread divider.
- Use focused browser/manual checks for actual scrolling, expansion, keyboard, and responsive behavior.

Manual verification should include:

- Desktop and mobile chat layout.
- Existing message history render.
- Unread divider anchor and clearing behavior.
- Loading older messages without jumpiness.
- Live reasoning/tool/question events.
- Question reply and reject.
- Empty, error, and attachment states.

## Rollout Plan

1. Introduce structural CSS and small rendering modules while preserving current visual behavior as much as possible.
2. Move message/activity/question rendering out of `wukong-chat.js` behind small functions or custom modules.
3. Apply the workbench layout and activity rail styling.
4. Verify existing Web Console chat tests and manual scroll behavior.
5. Iterate on polish only after the behavior is stable.

## Non-Goals

- Embedding opencode web with an iframe.
- Reverse proxying opencode web assets.
- Replacing Wukong's chat history database with opencode sessions.
- Letting browser code call `opencode serve` directly.
- Changing memory, skills, schedules, settings, or system APIs.
- Adding backend schema changes in the first UI pass.
- Streaming assistant text token-by-token.

## OpenCode Reference Boundaries

Allowed to borrow:

- Message part hierarchy.
- Session/thread framing.
- Activity/tool card interaction patterns.
- Keyboard-friendly composer conventions.
- Clearer inspectable agent progress affordances.

Not allowed to copy directly without a separate review:

- App runtime, router, state management, build tooling, or CSS framework.
- API ownership model.
- Authentication/session assumptions.
- Workspace filesystem controls.
- Exact visual branding.

# Issue 36 - Telegram 渲染：href 雙引號未跳脫、切段切斷實體、超長網址讓一則回答拆成數百則 (Small)

## 概述

`wukong-render` 的 `to_telegram_html`（`crates/wukong-render/src/lib.rs`）在 #35 之前就有三個問題，由 PR #35 第一輪第三方審查（https://github.com/raybird/Wukong/pull/35#issuecomment-6907242018 ）列出、#32 TBD-5 轉來。

2026-11-18 在 main（3c7630b）以 `to_telegram_html` 實跑（試跑程式在 session scratchpad 的 `repro36`，未進 repo）：

| 輸入 | 實際輸出 |
|---|---|
| `[docs](https://x.io/a"b)` | 1 段：`<a href="https://x.io/a"b">docs</a>`；2026-11-16 以 Bot API 實送回 `400 can't parse entities: Empty attribute name in the tag "a"`，#32 合併後退回純文字送達，連結不能點 |
| `a&` 重複 2100 次 | 4 段，每段 4096 bytes；段落邊界切出 `&am` 與 `p;`，Telegram 會照字面顯示 |
| `&` 重複 1500 次 | 2 段；第 1 段結尾是 `&`，第 2 段開頭是 `amp;` |
| 連結文字 `link text here`、網址 5000 字 | 15 段，每段 5029 bytes、只含 1 個連結文字字元 |
| 連結文字 300 個「字」、網址 4500 字 | 301 段，每段 4531 bytes、只含 1 個字 |
| 前後各一段文字、中間連結 `x`、網址 4090 字 | 4 段，第 2、3 段超過 4096 bytes，第 3 段是空的連結 |

原因：`escape_html` 只跳脫 `&`、`<`、`>`；`split_chunks` 以 byte 與字元邊界決定切點，沒有避開實體；切段時每段都要重開所有未關閉的標籤，`<a href="…">` 長度跟著網址走，網址一長，每段只剩一個字元的空間，而且重開的標籤本身就超過 4096。輸出的標籤只有 `<a href>` 長度不固定，其餘都是固定的短標籤。

受影響的呼叫端：Telegram 聊天回覆與排程結果回送（都經 `to_telegram_html`）。Web Console 用 `to_web_html`，不在範圍。

不在範圍：#39（回答提問後看不到處理進度）。

## 涉及檔案

- `crates/wukong-render/src/lib.rs` - 連結輸出、`split_chunks` 的切點與對應單元測試
- `CHANGELOG.md` - `[Unreleased]` Fixed 記錄使用者看得到的修正
- `docs/entrypoints.md` - Telegram「格式渲染」補上超長網址的連結改成純文字

## 驗收條件

1. **AC-1**：連結網址裡的雙引號在 href 內輸出成 `&quot;`，例如 `[docs](https://x.io/a"b)` 輸出 `<a href="https://x.io/a&quot;b">docs</a>`；一般文字的輸出不變 — 檢查方式：`cargo test -p wukong-render` 斷言完整輸出字串；以 Bot API 實送一次，Telegram 接受且連結指向 `https://x.io/a"b`
2. **AC-2**：切段不會把 `&amp;`、`&lt;`、`&gt;`、`&quot;` 切成兩半；切點落在實體中間時往前退到 `&` 之前，每段仍不超過 4096 bytes — 檢查方式：單元測試以概述表第 2、3 列斷言每段都不以實體的後半開頭、不以不完整的實體結尾，各段串接後與不切段的輸出相同
3. **AC-3**：跳脫後網址超過 2048 bytes 的連結不輸出 `<a>`，改寫成純文字「連結文字 (網址)」，和 #32 純文字退回的寫法一致，照一般文字切段；不超過 2048 bytes 的連結照原本輸出。任何輸入的每一段都不超過 4096 bytes — 檢查方式：單元測試以概述表第 4～6 列斷言每段不超過 4096 bytes、段數（5000 字網址的例子不超過 2 段）與輸出內容，另以 2048、2049 bytes 的網址斷言門檻兩側；以 Bot API 實送 2048 bytes 網址的連結與改成純文字的結果各一次，Telegram 皆接受
4. **AC-4**：既有行為不變：一般連結、`javascript:` 連結拿掉、表格、程式碼區塊、引言、清單、think 的既有測試全過 — 檢查方式：`cargo test -p wukong-render` 全綠，既有測試不改期望值

- **核准日期**: 2026-11-18
- **核准來源**: 使用者 2026-11-18 對話以「好，開始做 /dev-cycle #36」推進本 issue；同日 Q1「href 裡的雙引號怎麼處理」選「跳脫成 &quot; (Recommended)」→ AC-1；Q2「網址太長、無法和文字一起放進 4096 bytes 時怎麼辦」選「超過 2048 bytes 改純文字 (Recommended)」→ AC-3；Q3「驗收時可否用 RunWuKong 的 wukong-telegram bot 對自己的聊天實送測試訊息」選「可以實送 (Recommended)」→ AC-1、AC-3 的實送檢查；Q4「AC-2、AC-4 照建議核准」選「照建議核准 (Recommended)」→ AC-2、AC-4

**風險**: Low — 只改單一 crate，三個問題都已實跑重現（概述表）；Telegram 端的 `&quot;` 是 Bot API 文件列出的實體，以實送確認；既有輸出有回歸測試，容易回復。依一般相依順序實作與驗證。

## 實作與驗證步驟

1. 📝 **AC-4：改動前的回歸基準** — 驗證方式：記錄改動前 `cargo test -p wukong-render --locked` 的結果
   - 完成判準：既有測試在改動前全綠，數量已記錄
2. 📝 **AC-1～AC-3：先寫紅燈測試** — 驗證方式：`cargo test -p wukong-render --locked`，新測試在改動前失敗，失敗輸出對應概述表
   - 完成判準：記錄紅燈命令與輸出
3. 📝 **AC-1～AC-3：修改連結輸出與切點** — 驗證方式：步驟 1、2 的測試全綠
   - 完成判準：`cargo test -p wukong-render --locked` 全綠；修改前對每個要改的函式跑 GitNexus `impact` 並回報
4. 📝 **AC-1、AC-3：Telegram 實送** — 驗證方式：照 issue-0032 步驟 1 的做法，token 由 RunWuKong `wukong-telegram` 容器唯讀讀出、經 stdin 交給發送程式，只送到 allowed chat，不重啟、不改設定；以本分支的 `to_telegram_html` 產生 HTML 後送出 href 含 `&quot;` 的連結、2048 bytes 網址的連結、超過門檻改成純文字的結果
   - 完成判準：三則都回 `ok`，並確認第一則連結的網址是 `https://x.io/a"b`；記錄送出的 HTML 摘要與 API 回應
5. 📝 **交付檢查** — 驗證方式：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`；更新 `CHANGELOG.md` `[Unreleased]` Fixed 與 `docs/entrypoints.md`「格式渲染」
   - 完成判準：三道命令全綠，兩份文件已更新

## Timeline

| 日期 | 異動 | 負責人 |
|------|------|--------|
| 2026-11-16 | GitHub #36 建立（#32 TBD-5 另開） | - |
| 2026-11-18 | 建立 issue 文件；三項實跑重現；AC-1～AC-4 核准（Q1～Q4 照建議） | - |

---
**建立日期**: 2026-11-18  
**分級**: Small  
**風險**: Low\
**狀態**: 待實作

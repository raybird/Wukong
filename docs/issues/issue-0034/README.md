# Issue 34 - Telegram 渲染：HTML 屬性或註解裡的 think 字串被當成未關閉的 think，後面的回答整段被截掉 (Small)

## 概述

`wukong-render` 的 `strip_think`（`crates/wukong-render/src/lib.rs`，PR #33 引入）在 raw HTML 事件裡以 `<` 為起點找 think 的開始與結尾標籤，沒有區分真正的標籤與屬性值、註解裡的字串。屬性值或註解裡出現開始標籤的字串時，依「沒有結尾的拿掉到訊息結尾」的規則，後面的回答整段被刪掉。輸出本身是合法的 HTML，Telegram 不會拒收，#32 的純文字退回也接不住，使用者只是少看到內容。

2026-11-16 在 main（b676635）以 `to_telegram_html` 實跑（試跑程式在 session scratchpad 的 `repro34`，未進 repo）：

| 輸入 | 實際輸出 |
|---|---|
| `前文 <span title="<think>">文字</span>\n\n後續正常回答` | `前文 &lt;span title="` |
| `前文 <!-- <think> -->\n\n後續正常回答` | `前文 &lt;!--` |
| `<div title="<think>">\n\n後續正常回答` | `&lt;div title="` |
| `前文 <?pi <think> ?> 後\n\n答案` | `前文 &lt;?pi` |
| `前文 <![CDATA[<think>]]> 後\n\n答案` | `前文 &lt;![CDATA[` |

pulldown-cmark 把整個標籤（含屬性值）、整段註解、處理指令與 CDATA 各切成一個 HTML 事件，所以只要在事件內跳過引號內的屬性值與這幾種寫法，不需要完整的 HTML parser。同一個掃描也決定結尾標籤，所以反方向一樣有問題：think 區塊裡屬性值或註解帶結尾標籤時，think 會提早結束、推理內容外露。

受影響的呼叫端：Telegram 聊天回覆與排程結果回送（都經 `to_telegram_html`）。Web Console 用 `to_web_html`，不經 `strip_think`。

不在範圍：屬性值沒有結尾引號的寫法（見 TBD-1）；#36 的 href 雙引號、切段與長度問題。

## 涉及檔案

- `crates/wukong-render/src/lib.rs` - `strip_think` 的標籤辨識與對應單元測試
- `CHANGELOG.md` - `[Unreleased]` Fixed 記錄使用者看得到的修正

## 驗收條件

1. **AC-1**：HTML 屬性值（雙引號或單引號）裡的 think 開始或結尾標籤字串不算標籤；後面的回答完整輸出，該 HTML 標籤照原本的方式跳脫後顯示 — 檢查方式：`cargo test -p wukong-render` 以概述表前三列及單引號寫法斷言完整輸出字串
2. **AC-2**：HTML 註解 `<!-- … -->`、處理指令 `<?…?>`、CDATA `<![CDATA[…]]>`、宣告 `<!…>` 裡的 think 字串不算標籤；後面的回答完整輸出，這些寫法照原本跳脫後顯示 — 檢查方式：單元測試以行內與區塊兩種位置斷言完整輸出字串
3. **AC-3**：think 區塊內，屬性值或 AC-2 寫法裡的結尾標籤字串不會讓 think 提早結束；think 內容（含那個屬性或註解）不輸出，真正的結尾標籤之後的文字照常輸出 — 檢查方式：單元測試斷言完整輸出字串，且輸出不含 think 內文
4. **AC-4**：既有行為不變：issue-0031 AC-3 的 think 測試（獨立標籤、表格儲存格、跨引言或清單邊界、巢狀、連續、沒有結尾、程式碼內保留）全過；HTML 區塊裡的獨立 think 標籤照樣連同內容拿掉 — 檢查方式：`cargo test -p wukong-render` 全綠，另加 HTML 區塊案例（`<div>\n前文 <think>x</think> 後\n</div>\n\n答案`，改動前輸出 `&lt;div&gt;\n前文  後\n&lt;/div&gt;\n答案`）在改動前後都通過

- **核准日期**: 2026-11-16
- **核准來源**: 使用者 2026-11-16 對話以「好依照建議 /dev-cycle #34」推進本 issue；同日 Q1「處理指令、CDATA、宣告裡的 think 是否一起排除」選「一起排除 (Recommended)」→ AC-2；Q2「AC-1～AC-4 照建議核准」選「照建議核准 (Recommended)」→ AC-1～AC-4

**風險**: Low — 只改單一 crate 的單一函式，行為由 pulldown-cmark 的事件切法決定，已於 2026-11-16 實跑確認（概述表）；既有 think 行為有回歸測試，容易回復。依一般相依順序實作與驗證。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|------|------|------|------|
| TBD-1 | 屬性值沒有結尾引號（例如 `<div title="<think>` 後面沒有 `">`）時，依 CommonMark 這不是合法標籤，pulldown-cmark 把前半當文字、把 think 開始標籤當成一個獨立標籤，仍會截掉後面的回答 | 不影響本次交付 | 追蹤：無——從 Markdown 看它就是獨立標籤，要分辨得猜測模型的意圖；AC-1～AC-4 的案例都是合法寫法 |

## 實作與驗證步驟

1. ⏳ **AC-4：改動前的回歸基準** — 驗證方式：加入 HTML 區塊獨立 think 的案例，在改動前的程式上通過；記錄 `cargo test -p wukong-render` 改動前結果
   - 完成判準：新案例與既有 think 測試在改動前全綠
2. ⏳ **AC-1～AC-3：先寫紅燈測試** — 驗證方式：`cargo test -p wukong-render`，新測試在改動前失敗，失敗輸出對應概述表的截斷或 think 內文外露
   - 完成判準：記錄紅燈命令與輸出
3. ⏳ **AC-1～AC-3：修改 `strip_think`** — 驗證方式：步驟 1、2 的測試全綠
   - 做法：在每個 raw HTML 事件的片段內由左往右掃描；遇到註解、處理指令、CDATA、宣告就跳到各自的結尾（片段內沒有結尾時跳到片段結尾）；遇到標籤時只在標籤名稱處判斷是否為 think 開始或結尾，再跳過引號內的屬性值直到標籤結束
   - 完成判準：`cargo test -p wukong-render` 全綠
4. ⏳ **交付檢查** — 驗證方式：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`；更新 `CHANGELOG.md` `[Unreleased]` Fixed
   - 完成判準：三道命令全綠，CHANGELOG 已更新

## Timeline

| 日期 | 異動 | 負責人 |
|------|------|--------|
| 2026-11-16 | GitHub #34 建立（#31 TBD-7 另開） | - |
| 2026-11-16 | 建立 issue 文件；AC-1～AC-4 核准（Q1、Q2 照建議） | - |

---
**建立日期**: 2026-11-16  
**分級**: Small  
**風險**: Low\
**狀態**: 待實作

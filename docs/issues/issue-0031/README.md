# Issue 31 - Telegram 渲染：子清單黏在父項同一行、編號變成 •、標題不分層級 (Small)

## 概述

`wukong_render::to_telegram_html` 把 Markdown 轉成 Telegram HTML 時，清單、標題、`<think>` 與巢狀引言的處理有缺陷。2026-11-13 以下列樣本實跑（試跑程式在 session scratchpad 的 `rendercheck`，未進 repo）：

```markdown
3. 第三步
4. 第四步
   - 子項 A
     - 孫項
   - 子項 B
```

輸出：

```
• 第三步
• 第四步• 子項 A• 孫項

• 子項 B
```

- 子清單的第一項沒有換行，直接接在父項後面；沒有縮排，看不出層級。
- 編號清單的號碼全變成 •。
- `#` 到 `######` 全部只輸出 `<b>`。
- `<think>…</think>` 跳脫成 `&lt;think&gt;…` 原樣顯示。
- 巢狀引言輸出兩層 `<blockquote>`。Telegram HTML 接不接受尚未驗證。

受影響的呼叫端：`crates/wukong-telegram/src/dispatch.rs:1509`（聊天回覆）、`crates/wukong-schedulerd/src/notify.rs:78`（排程結果回送）。Web Console 用 `to_web_html`，不在範圍內。

`send_message_html`（`crates/wukong-tg-client/src/client.rs:205`）收到 400 時只記 log，不會退回純文字，所以渲染輸出只要 Telegram 不接受，那一段回覆就會遺失。

參考：moltbot-lite issue 0027 在 MarkdownV2 模式修過同一類問題，做法與實機踩到的坑由該 session 於 2026-11-13 提供。

不在範圍：表格、連結、程式碼區塊、長訊息切段（`split_chunks`）的既有行為；行內 HTML（例如 `<u>`）照現況跳脫顯示；400 時退回純文字（見 TBD-4）。

## 涉及檔案

- `crates/wukong-render/src/lib.rs` - `render_html` 的清單、標題、引言、`<think>` 處理與對應單元測試
- `CHANGELOG.md` - `[Unreleased]` 記錄使用者看得到的排版變更
- `docs/entrypoints.md` - Telegram「格式渲染」一行補上清單層級、編號與標題

## 驗收條件

1. **AC-1**：清單每一項都另起一行；子清單每層多縮 2 個空白，符號依層級為 •、◦、▪（第 4 層以後沿用 ▪）；編號清單保留原始號碼（`3.`、`4.`），巢狀的編號清單也一樣— 檢查方式：`cargo test -p wukong-render` 中以上方樣本斷言完整輸出字串
   - 失敗路徑：任一子項與父項在同一行、號碼被換成符號、縮排與層級不符，測試紅燈
2. **AC-2**：H1 輸出 `<u><b>…</b></u>`（底線在粗體外層），H2～H6 輸出 `<b>…</b>`— 檢查方式：單元測試斷言兩種層級的輸出
   - 失敗路徑：H1 與 H2 輸出相同，或 `<u>`、`<b>` 巢狀順序顛倒
3. **AC-3**：`<think>…</think>` 連同內容不輸出，前後文字照常輸出；區塊與行內兩種寫法都適用，表格儲存格內、跨引言或清單邊界、巢狀與連續的 think 也一樣，且輸出的 HTML 標籤仍成對；行內程式碼與程式碼區塊裡的 think 標籤是內容，照原本跳脫後保留；沒有結尾的 `<think>` 從該處拿掉到訊息結尾— 檢查方式：單元測試斷言完整輸出字串
   - 失敗路徑：輸出出現 `&lt;think&gt;` 或 think 內文（程式碼內除外），前後文字一併被吃掉，或 `<blockquote>` 等標籤沒有關閉
4. **AC-4**：巢狀引言攤平成單層 `<blockquote>`，內文全部保留— 檢查方式：單元測試斷言輸出只有一組 `<blockquote>`…`</blockquote>`，且兩層的文字都在
   - 失敗路徑：輸出仍有兩層 `<blockquote>`，或內層文字遺失
5. **AC-5**（覆蓋最大風險）：AC-1～AC-4 樣本的實際輸出，以 Bot API `sendMessage`（`parse_mode=HTML`）送到使用者自己的聊天，每則都回 `ok: true`，手機上看得到縮排— 檢查方式：保存每則請求的 HTML 與 API 回應，加上使用者手機確認
   - 失敗路徑：任一則回 400，或行首縮排被 Telegram 吃掉；後者改用不換行空白（U+00A0）縮排後重送
6. **AC-6**：不含清單、標題、`<think>`、巢狀引言的輸入，輸出與改動前逐字相同；既有 `wukong-render` 測試全綠— 檢查方式：改動前先對一組樣本記錄輸出快照，改動後同組比對；`cargo test -p wukong-render`
   - 失敗路徑：任一則快照不同，或既有測試紅燈
7. **AC-7**：清單項目裡的接續段落與換行，縮到該項文字的位置（項目符號或編號之後），子清單項目裡的也一樣— 檢查方式：單元測試斷言鬆散清單（含子清單的第二段）與兩位數編號接續行的完整輸出字串
   - 失敗路徑：接續段落或換行後的文字貼在行首，或縮排與該項文字的位置不符

- **核准日期**: 2026-11-13
- **核准來源**: 使用者 2026-11-13 對話同意「替第 1 項（Telegram 渲染）開 issue」（回覆「好」），範圍為子清單換行與縮排、保留編號、標題層級、`<think>`、巢狀引言；同日對 AC-1～AC-6 的建議格式（縮排 2 空白與 •◦▪、H1 加底線、think 整段拿掉、巢狀引言攤平、實機送出、逐字回歸）與 TBD-4 另開 issue 的建議回覆「都照建議」
- **修訂核准**: 2026-11-16，使用者 2026-11-16 對 PR #33 審查後的 TBD-5、TBD-6 建議回覆「好這個 issue 一起修」：AC-3 補上表格、跨區塊、巢狀、程式碼內與沒有結尾的處理（TBD-5），新增 AC-7（TBD-6）
- **核准 commit**: 待提交

## 風險與首要驗證

- **最大風險**：Telegram HTML 模式的實際契約未驗證——行首空白會不會被吃掉、`<u><b>` 等組合接不接受。渲染輸出被拒時，`send_message_html` 不會退回純文字，那段回覆直接遺失，使用者收不到任何東西
- **風險等級與理由**：Medium。外部 API 行為有一項重要未知；失敗後果是訊息遺失，但只影響 Telegram 路徑，改動侷限在單一 crate、容易回復
- **首要驗證**：主要實作前，先用 Bot API `sendMessage` 對使用者自己的聊天送 4 則手寫樣本（普通空白縮排、U+00A0 縮排、兩層 `<blockquote>`、`<u><b>…</b></u>`）
- **選擇理由**：這幾個問題只有 Telegram 伺服器能回答，單元測試與文件都無法證明；moltbot-lite 只在 MarkdownV2 實測過，HTML 模式沒有可沿用的結果
- **完成證據**：4 則請求的 HTML 原文與 API 回應（`ok` 或錯誤描述）記入步驟 1，加上手機畫面上縮排是否可見

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|------|------|------|------|
| TBD-1 | AC-1～AC-6 的格式建議值（縮排 2 空白與 •◦▪、H1 加底線、think 整段拿掉、巢狀引言攤平）待使用者核准或修改 | 已解決 | 2026-11-13 使用者回覆「都照建議」，AC-1～AC-6 照建議值核准 |
| TBD-2 | Telegram HTML 模式下行首空白是否保留、兩層 `<blockquote>` 是否接受 | 已解決 | 2026-11-15 實機：行首一般空白保留（`sendMessage` 回傳的 `text` 逐字含 2／4 個空白），`LIST_INDENT` 維持一般空白；兩層 `<blockquote>` 回 `ok: true`，所以 AC-4 的攤平不是避免 400 的必要條件，維持已核准的攤平行為 |
| TBD-3 | 回歸語料來源：要不要從部署的聊天歷史撈真實回覆做新舊對照（moltbot-lite 的做法），以及要撈哪一份部署的資料（project.md 寫 `~/Documents/RunWuKong` 是實際部署，但另有紀錄指出它是本機測試部署） | 已解決 | 2026-11-13 使用者「TBD-3 照建議」：快照以手寫樣本為主，另以唯讀方式撈 `~/Documents/RunWuKong` 聊天歷史的 assistant 回覆當補充語料（不論是否為正式部署，內容都是真實模型輸出）；原文只放暫存目錄，不進 repo |
| TBD-5 | 沒有結尾的 `<think>`（模型輸出被截斷或格式錯誤）怎麼處理。PR #33 審查要求明確定義。建議：從 `<think>` 拿掉到訊息結尾，寧可少顯示也不外露推理；整則都被拿掉時，聊天回覆照既有邏輯送「(無內容)」 | 已解決 | 2026-11-16 使用者 2026-11-16 對 PR #33 審查後的 TBD-5、TBD-6 建議回覆「好這個 issue 一起修」，照建議寫進 AC-3。c656c35 的實作本來就是這個行為，只是沒寫進 AC |
| TBD-6 | 清單項目裡的第二段（鬆散清單的接續段落）沒有縮排，看不出屬於哪一項，例如 `• 第一項\n\n第一項第二段\n\n  ◦ 子項\n\n子項第二段`。PR #33 審查列為 P3 額外覆蓋。建議：本 issue 內一起修，接續段落縮到該項文字的位置 | 已解決 | 2026-11-16 使用者 2026-11-16 對 PR #33 審查後的 TBD-5、TBD-6 建議回覆「好這個 issue 一起修」，新增 AC-7 |
| TBD-4 | Telegram 回 400 時退回純文字的備援，會動到 `wukong-tg-client`、`wukong-telegram`、`wukong-schedulerd`，是否另開 issue | 不影響本次交付 | 追蹤：#32（2026-11-13 使用者照建議另開）。本次 AC 只改渲染輸出，AC-5 以實機送出確認不會被拒 |

## 實作與驗證步驟

1. ✅ **AC-5 前置：實機探測 Telegram HTML 契約** — 驗證方式：以 `curl` 呼叫 Bot API `sendMessage`（`parse_mode=HTML`）送 4 則手寫樣本到使用者自己的聊天，token 只經環境變數傳入
   - 產出：每則的 HTML 原文、API 回應、手機上縮排是否可見；據此回答 TBD-2
   - 相依：需要使用者提供可用的 bot token 與 chat id
   - 完成判準：4 則都有記錄回應，TBD-2 改為 `已解決`
   - 完成證據（2026-11-15）：經使用者同意（「可以使用 RunWukong 的 docker compose 模擬測試」），以 RunWuKong `wukong-telegram` 容器 `/data/settings.json` 的 bot token 與 allowed chat 發送；token 經 stdin 交給 `curl --config -`，未出現在命令列或檔案。結果：一般空白縮排 `ok`、U+00A0 縮排 `ok`、兩層 `<blockquote>` `ok`、`<u><b>…</b></u>` `ok`。另送一則合併樣本檢查回傳的 `result.text`：`'  ◦ 一般空白兩格'`、`'    ▪ 一般空白四格'`，行首空白保留。第一次發送時回應解析腳本有語法錯誤，第 1 則重送一次
2. ✅ **AC-6：改動前的輸出快照** — 驗證方式：對一組不含受影響結構的樣本（段落、粗斜體、行內程式碼、程式碼區塊、表格、連結、單層引言、長文切段）記錄目前的 `to_telegram_html` 輸出，寫成測試
   - 補充語料：唯讀撈 `~/Documents/RunWuKong` 聊天歷史的 assistant 回覆，新舊輸出對照；不含受影響結構的回覆須逐字相同，含受影響結構的回覆檢查原文字詞沒有遺失
   - 完成判準：快照測試在改動前的程式上通過
   - 完成證據（2026-11-13）：`telegram_output_unchanged_for_unaffected_structures` 7 組樣本，期望值取自改動前程式的實際輸出；改動前 `cargo test -p wukong-render` 24 passed。補充語料：唯讀 `docker cp` 出 RunWuKong `wukong-telegram` 容器的 `/data/memory.db`，`chat_messages` 只有 4 則 assistant 回覆（2026-09～10），排程沒有任何 run 紀錄；改動前後輸出 4 則逐字相同（其中 2 則含頂層清單）。原文只放 session scratchpad，未進 repo
3. ✅ **AC-1～AC-4：先寫紅燈測試** — 驗證方式：`cargo test -p wukong-render`，新測試在改動前失敗，失敗訊息對應各 AC 的失敗路徑
   - 相依：AC-1、AC-4 的細節依步驟 1 結果
   - 完成判準：記錄紅燈命令與輸出
   - 完成證據（2026-11-13）：`cargo test -p wukong-render` 24 passed、4 failed，各自對應失敗路徑——`telegram_flattens_nested_blockquotes`（`<blockquote>` 出現 2 次）、`telegram_h1_is_underlined_bold_and_lower_levels_are_bold`（H1 輸出 `<b>一級</b>`）、`telegram_nested_lists_keep_lines_levels_and_numbers`（實際 `• 第三步\n• 第四步• 子項 A• 孫項\n\n• 子項 B`）、`telegram_drops_think_blocks_and_keeps_surrounding_text`（think 標籤外露）
4. ✅ **AC-1～AC-4：修改 `render_html`** — 驗證方式：步驟 2、3 的測試全綠
   - 完成判準：`cargo test -p wukong-render` 全綠
   - 完成證據（2026-11-13）：28 passed、0 failed。修改前 GitNexus `impact(render_html, upstream)` 回 CRITICAL：直接呼叫者只有 `to_telegram_html`，13 條受影響流程都在 Telegram 回覆與排程回送路徑，與本 issue 範圍一致，已在對話中告知使用者。縮排先用一般空白（`LIST_INDENT`），待步驟 1 實機結果決定是否改 U+00A0。NOTE：原排序是步驟 1 實機探測在前；因 token 尚未提供，先完成不依賴探測結果的實作，探測結果只會影響 `LIST_INDENT` 一個常數
5. ✅ **AC-5：實機送出實作後的輸出** — 驗證方式：把 AC-1～AC-4 樣本經 `to_telegram_html` 的實際輸出送到使用者聊天
   - 完成判準：每則都回 `ok: true`，使用者確認手機上的排版
   - 完成證據（2026-11-15）：AC-1～AC-4 合併樣本（H1／H2、編號清單含兩層子清單、無序清單含編號子清單、巢狀引言、單獨一行的 `<think>`）經改動後的 `to_telegram_html` 輸出，`sendMessage` 回 `ok`。2026-11-15 使用者手機確認排版：「沒問題」
6. ✅ **交付檢查** — 驗證方式：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`；更新 `CHANGELOG.md` 的 `[Unreleased]` 與 `docs/entrypoints.md` 的「格式渲染」
   - 完成判準：三道命令全綠，兩份文件已更新
   - 完成證據（2026-11-13，步驟 1、5 之前）：`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 無警告；`cargo test --workspace --locked` 723 passed、0 failed；`CHANGELOG.md` `[Unreleased]` Fixed 與 `docs/entrypoints.md`「格式渲染」已更新。步驟 1 結果未改動 `LIST_INDENT`，不需重跑

7. ✅ **PR #33 審查修正（think 過濾）** — 驗證方式：先把審查的重現案例寫成測試、在 c656c35 的實作上紅燈，改完後全綠
   - 審查：2026-11-16 第三方審查 https://github.com/raybird/Wukong/pull/33#issuecomment-6905285202 ，判定 REQUEST_CHANGES：P1 表格儲存格內的 think 外露、P1 think 跨引言或清單邊界時跳過結構的 End 事件、P2 巢狀 think 只認第一個結尾
   - 做法：think 過濾從 `render_html` 的事件迴圈移出，改在解析前處理——先解析一次，只在 raw HTML 事件裡依位置找 `<think>`／`</think>`（以深度計數處理巢狀），把這些範圍從 Markdown 原文剪掉，再解析剩下的文字渲染。renderer 只看到完整的文件，標籤自然成對；程式碼內的標籤不是 raw HTML 事件，不會被剪。原本為 think 加的「空段落不輸出空行」（`paragraph_start`）已用不到，一併拿掉，raw HTML 跳脫恢復成 main 的寫法
   - 完成證據（2026-11-16）：新增 `telegram_drops_think_inside_table_cells`、`telegram_think_across_blocks_keeps_html_balanced`、`telegram_drops_nested_consecutive_and_unclosed_think`、`telegram_keeps_think_tags_inside_code`。套在 c656c35 的實作上 29 passed、3 failed（表格輸出 `A   hidden reasoning答案`；引言輸出 `<blockquote>引言 \n正常回答`，沒有 `</blockquote>`；巢狀輸出 `剩餘推理\n&lt;/think&gt;\n最終答案`）；程式碼那組在舊實作上本來就通過，留作防護。改完後 `cargo test -p wukong-render` 32 passed；`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets -- -D warnings` 無警告；`cargo test` 727 passed、0 failed
   - 完成判準：TBD-5 有答案，AC-3 重新核准（2026-11-16 已核准）
   - NOTE（2026-11-16）：步驟 4 的 think 處理（事件迴圈內以布林狀態過濾）已由本步驟取代

8. ✅ **AC-7：清單項目的接續行縮排** — 驗證方式：先寫紅燈測試，再修改 `render_html`；改動後的輸出實機送出
   - 做法：以堆疊記錄每個開著的清單項目文字起始欄（縮排長度加符號或編號的字元數）；項目裡不是第一行的段落，以及段落內的換行，補上這個欄數的空白
   - 完成證據（2026-11-16）：`telegram_indents_continuation_lines_inside_list_items` 改動前紅燈（實際 `• 第一項\n\n第一項第二段\n\n  ◦ 子項\n\n子項第二段\n\n• 第二項`），改動後 `cargo test -p wukong-render` 33 passed；`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets -- -D warnings` 無警告；`cargo test` 728 passed、0 failed
   - AC-5 補驗（2026-11-16）：步驟 7、8 改變了輸出（接續行開頭的空白、跨引言時補上 `</blockquote>`），把鬆散清單、兩位數編號接續行、think 跨引言、表格內 think 合併成一則，經改動後的 `to_telegram_html` 輸出以 RunWuKong bot 送出，`sendMessage` 回 `ok: true`；回傳的 `result.text` 保留行首空白：`'  第一項第二段'`、`'  ◦ 子項'`、`'    子項第二段'`、`'    說明'`。token 由 `docker exec` 讀出後經 stdin 交給 Python，未出現在命令列或檔案

## Timeline

| 日期 | 異動 | 負責人 |
|------|------|--------|
| 2026-11-13 | 建立（GitHub #31） | - |
| 2026-11-13 | AC-1～AC-6 核准（「都照建議」）；400 備援另開 #32 | - |
| 2026-11-16 | PR #33 第三方審查 REQUEST_CHANGES（P1 ×2、P2 ×1）；AC-3 補上表格、跨區塊、巢狀、程式碼內與未關閉的處理；同日 TBD-5、TBD-6 照建議核准（「好這個 issue 一起修」），新增 AC-7 | - |

---
**建立日期**: 2026-11-13  
**分級**: Small  
**風險**: Medium\
**狀態**: 實作中

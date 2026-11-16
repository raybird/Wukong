# Issue 32 - Telegram 回 400 時 HTML 回覆整段遺失，沒有退回純文字

## 概述

`send_message_html`（`crates/wukong-tg-client/src/client.rs:205`）以 `parse_mode=HTML` 送出。Telegram 判定 HTML 不合法、回 400 時，`check_ok` 會把它轉成 `TgError::Api`，兩個呼叫端的處理方式如下：

- 聊天回覆（`crates/wukong-telegram/src/dispatch.rs:1514`）：`log_send` 記一行 stderr，接著送下一段。使用者完全收不到那一段，也看不到錯誤提示。
- 排程結果回送（`crates/wukong-schedulerd/src/notify.rs:86`）：用 `?` 往上回傳，所以第一個被拒的段落之後，剩下的段落也都不送。

本 issue 讓 HTML 解析失敗的那一段，改用純文字重送同一段內容。來源：#31 規劃時發現（#31 README TBD-4），2026-11-13 另開。

不在範圍：HTML 解析失敗以外的錯誤（chat not found、網路錯誤等）照現況處理；`wukong-render` 的渲染輸出不改；Web Console 不受影響。

## 文件清單
- [implementation-plan.md](./implementation-plan.md)

## 快速導覽

### 當前流程
`to_telegram_html` 切段 → 逐段 `send_message_html` → 被拒的段落遺失（聊天）或連後續段落一起遺失（排程）。

### 修改後流程
`to_telegram_html` 切段 → 逐段送出 HTML；只有 HTML 解析失敗時，把該段 HTML 轉成純文字，用 `send_message` 重送，並記一行 stderr 警告 → 繼續送下一段。

## 關鍵差異
| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| 聊天回覆某段 HTML 被拒 | 該段遺失 | 該段改以純文字送達 |
| 排程回送某段 HTML 被拒 | 該段與之後的段落都遺失，回報錯誤 | 該段改以純文字送達，後續段落照常送出 |
| 非解析錯誤（chat not found、網路錯誤等） | 聊天記 log 後送下一段；排程中止 | 不變 |
| stderr | `Telegram 傳送失敗（answer）：…`（聊天）／排程 delivery 警告 | 退回時另記一行：Telegram 錯誤描述與呼叫端，不含訊息內容 |

## 涉及檔案
### wukong-tg-client
- `crates/wukong-tg-client/src/client.rs` - 判斷 HTML 解析失敗、HTML 轉純文字、帶退回的送出函式；mock 加上「拒收 HTML」的設定
- `crates/wukong-tg-client/src/error.rs` - 若判斷放在 `TgError` 上，加在這裡

### 呼叫端
- `crates/wukong-telegram/src/dispatch.rs` - 聊天回覆改用帶退回的送出
- `crates/wukong-schedulerd/src/notify.rs` - 排程結果回送改用帶退回的送出

### 常青文件
- `docs/entrypoints.md` - Telegram「格式渲染」補上退回行為
- `AGENTS.md` - 「關鍵設計細節」的「排程結果回送」補上退回行為
- `CHANGELOG.md` - `[Unreleased]` Fixed

## Gherkin 驗收劇本

```gherkin
Feature: Telegram 拒收 HTML 時改以純文字送達

  Background:
    Given 回覆經 to_telegram_html 渲染成一或多段 HTML

  @SCN-001
  Scenario: 聊天回覆的某段 HTML 被判定解析失敗
    Given 一則分成 3 段的聊天回覆，Telegram 對第 2 段的 HTML 回 400、錯誤描述屬於 HTML 解析失敗
    When Wukong 送出這則回覆
    Then 第 1、3 段以 HTML 送出
    And 第 2 段以純文字（不帶 parse_mode）送出，內容為該段 HTML 轉成的純文字
    And 使用者收到全部 3 段

  @SCN-002
  Scenario: 排程結果回送的某段 HTML 被判定解析失敗
    Given 一個 Telegram 建立的排程，成功結果分成 2 段，Telegram 對第 1 段的 HTML 回 400、錯誤描述屬於 HTML 解析失敗
    When schedulerd 回送結果
    Then 第 1 段以純文字送出，第 2 段以 HTML 送出
    And 回送結果為已送出，不回報錯誤

  @SCN-003
  Scenario Outline: HTML 轉成純文字
    Given 一段被拒的 HTML <輸入>
    When 轉成純文字重送
    Then 送出的文字為 <輸出>

    Examples:
      | 輸入                                         | 輸出                          |
      | <b>粗</b> <i>斜</i> <code>x</code>           | 粗 斜 x                       |
      | a &lt; b &amp;&amp; c &gt; d                 | a < b && c > d                |
      | <a href="https://x.io/?a=1&amp;b=2">docs</a> | docs (https://x.io/?a=1&b=2)  |
      | 第一行\n  ◦ 子項\n<blockquote>引言</blockquote> | 第一行\n  ◦ 子項\n引言        |

  @SCN-004
  Scenario: 退回時留下可追查的紀錄
    Given 某段 HTML 被判定解析失敗並改送純文字
    When 退回發生
    Then stderr 多一行警告，含呼叫端（聊天回覆或排程回送）與 Telegram 的錯誤描述
    And 這行警告不含訊息內容
    And 使用者收到的訊息不加任何退回註記

  @SCN-005
  Scenario Outline: 不是 HTML 解析失敗的錯誤不重送
    Given Telegram 對某段 HTML 回 <錯誤>
    When Wukong 送出這段
    Then 不以純文字重送
    And 聊天回覆照現況記 log 後送下一段，排程回送照現況中止並回報錯誤

    Examples:
      | 錯誤                          |
      | 400 Bad Request: chat not found |
      | 403 Forbidden                 |
      | 網路錯誤                      |

  @SCN-006
  Scenario: 純文字重送也失敗
    Given 某段 HTML 被判定解析失敗，純文字重送也回錯誤
    When Wukong 送出這段
    Then 聊天回覆記 log 後送下一段
    And 排程回送中止後續段落並回報純文字重送的錯誤

  @SCN-007
  Scenario: HTML 被接受時行為不變
    Given Telegram 接受每一段 HTML
    When Wukong 送出聊天回覆或排程結果
    Then 每段只以 HTML 送出一次，沒有純文字重送，也沒有退回警告

  @SCN-008
  Scenario: 實機確認解析失敗的判斷與退回
    Given 使用 RunWuKong wukong-telegram 容器的 bot 與使用者自己的聊天
    When 送出格式錯誤的 HTML（不支援的標籤、沒有結尾的標籤、不合法的實體），以及一則會回 400 但不是解析失敗的對照請求
    Then 格式錯誤的那幾則被判定為解析失敗，對照請求不被判定為解析失敗
    And 經帶退回的送出函式送出格式錯誤的 HTML 時，純文字重送回 ok: true
```

## Gherkin 核准紀錄
- **核准 commit**: 待提交
- **核准來源**: 使用者 2026-11-16 對話以 `/dev-cycle issue:32` 推進本 issue，同日對四題建議全部照選：觸發條件「只限 HTML 解析失敗」（純文字也失敗時兩個呼叫端維持現況）→ SCN-001、SCN-002、SCN-005、SCN-006；純文字內容「把該段 HTML 轉成純文字」→ SCN-003；告知方式「只記一行 stderr 警告」→ SCN-004；實機探測「可以，照 #31 做法」→ SCN-008。SCN-007 為現況不變的回歸條件

全部 Scenario 於 2026-11-16 核准。

## 風險與首要驗證

- **最大風險**：「HTML 解析失敗」的判斷依據是 Telegram 錯誤描述的字樣（預期為 `400` 加 `Bad Request: can't parse entities: …`），這個字樣還沒實測。判斷太窄，實際的解析錯誤不會退回，問題原樣存在而單元測試全綠（mock 的錯誤字串是自己寫的）；判斷太寬，非解析錯誤也被重送
- **風險等級與理由**：Medium。外部 API 的錯誤契約有一項重要未知；失敗後果是退回沒有作用（回到現況），不會造成資料損失，改動沿既有相依方向、容易回復
- **首要驗證**：主要實作前，用 Bot API `sendMessage`（`parse_mode=HTML`）對使用者自己的聊天送 3 則格式錯誤的 HTML（不支援的標籤、沒有結尾的標籤、不合法的實體），加 1 則會回 400 但不是解析失敗的對照請求，記錄 `error_code` 與 `description`
- **選擇理由**：錯誤字樣只有 Telegram 伺服器能回答；mock 測試只能驗證自己寫進去的字串。對照請求證明判斷不會把其他 400 一起算進去
- **完成證據**：4 則請求的 HTML 原文與 API 回應記入 implementation-plan 步驟 1，判斷條件依實際字樣寫定

## Timeline

| 日期 | 異動 | 負責人 |
|------|------|--------|
| 2026-11-13 | GitHub #32 建立（#31 TBD-4 另開） | - |
| 2026-11-16 | 建立 issue 文件；SCN-001～SCN-008 核准（四題照建議） | - |

---
**建立日期**: 2026-11-16  
**分級**: Medium — 跨 `wukong-tg-client`、`wukong-telegram`、`wukong-schedulerd`，沿既有相依方向，沒有 schema 變更（project.md 規模判定）  
**風險**: Medium\
**狀態**: 待實作

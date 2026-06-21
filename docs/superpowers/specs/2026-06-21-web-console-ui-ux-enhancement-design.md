# Web Console (wukong-web) UI/UX 體驗提升設計規格書

**日期:** 2026-06-21
**狀態:** 提案中 (Proposal)
**前置:** `wukong-web` 既有實作、`static/styles.css`、`static/components/wukong-chat.js`。

---

## 目標

針對 Wukong 的 Web 控制台進行深度的 UI/UX 與視覺設計重構。將現有的陽春、預設瀏覽器樣式（Plain Vanilla），升級為具備**孫悟空品牌美學（東方神話結合現代科技暗色系）**、**極致實用性（多行輸入、程式碼高亮與複製）**與**優雅微互動（平滑展開、骨架屏與過渡動畫）**的 Premium 個人 AI 助理介面。

本設計方案持續遵循 `plainvanillaweb` 的核心指導精神：**零建置步驟**、純原生 CSS、純原生 JS（Web Components），不引入額外繁重的 Node 打包工具或 JavaScript 框架。

---

## 設計原則

1. **大聖品牌視覺 (Visual Identity)**: 引入專屬的色彩調色盤（大聖金紅），搭配漸層、微陰影與毛玻璃效果（Glassmorphism），讓第一眼視覺呈現高級質感（Wow Effect）。
2. **多行自適應輸入 (Multiline Composer)**: 徹底解決目前只能單行輸入的問題，提供自適應高度的程式碼/文字輸入區域。
3. **優雅的思考渲染 (Polished Thinking Display)**: 重構 `<details>` 推理日誌，加入流暢的展開與收納動畫，並優化等待期間的視覺回饋。
4. **極簡程式碼輔助 (Code Utilities)**: 為 AI 輸出的程式碼區塊（Code Block）加入一鍵複製功能與精美代碼樣式。
5. **平滑的狀態過渡 (Micro-interactions)**: 在歷史載入、發送對話、思考狀態下引入微動畫與骨架屏（Skeleton Loading），消除視覺跳動產生的生硬感。

---

## 詳細設計方案

### 1. CSS 設計系統與視覺重構 (`styles.css`)

為了讓介面具備一致性且易於維護，引入 CSS 自定義屬性（Variables）來打造精緻的「大聖主題」配色系統：

```css
:root {
  /* 設計標記：以暗灰色為底，搭配大聖橘紅、金黃 */
  --bg-primary: #121316;
  --bg-secondary: #1a1c23;
  --bg-tertiary: #242835;
  
  --accent-sun: #ea580c;       /* 觔斗雲橘紅 */
  --accent-gold: #eab308;      /* 火眼金睛金黃 */
  --text-primary: #f3f4f6;
  --text-secondary: #9ca3af;
  
  --bubble-user: linear-gradient(135deg, #ea580c 0%, #d97706 100%);
  --bubble-assistant: #1f2937;
  --border-color: rgba(255, 255, 255, 0.08);
  --border-radius: 12px;
  
  --glass-effect: backdrop-filter: blur(16px) saturate(180%);
  --transition-smooth: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  --font-modern: 'Outfit', 'Inter', system-ui, sans-serif;
}
```

* **字型引入**: **不加載任何外部網路字型服務（如 Google Fonts）**以維持完全離線執行能力。字體設定採用系統原生內建字型（System Fonts Fallback List，如 Linux 的 `Ubuntu`, `Liberation Sans`，macOS/Windows 的 `SF Pro`, `Segoe UI`，以及無襯線系統字體），兼顧讀取速度與視覺質感。
* **毛玻璃導覽列 (Glassmorphism Header)**: 
  * Header 採用 `background: rgba(26, 28, 35, 0.75)`，搭配 `backdrop-filter: blur(12px)`。
  * 精簡的邊框設計與半透明效果，使用 `box-shadow: 0 4px 30px rgba(0, 0, 0, 0.1)` 浮現於對話流上方。

---

### 2. 自適應多行輸入框與快捷鍵 (`wukong-chat.js`)

將目前的單行 `<input id="q" type="text" />` 重構為動態 Textarea 容器：

```html
<!-- 新的 Composer HTML -->
<form id="form" class="composer">
  <div class="textarea-wrapper">
    <textarea id="q" rows="1" placeholder="問悟空… (Enter 送出, Shift+Enter 換行)"></textarea>
  </div>
  <button type="submit" class="send-btn">
    <svg class="send-icon" viewBox="0 0 24 24">
      <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z"/>
    </svg>
  </button>
</form>
```

#### JavaScript 動態高度計算 (Auto-resize Logic)
在 JS 中加入高度自適應邏輯，最大高度限制為 `200px`，超過時自動出現內部滾動：

```javascript
const textarea = this.querySelector('#q');
textarea.addEventListener('input', function() {
  this.style.height = 'auto';
  this.style.height = (this.scrollHeight) + 'px';
});

// 監聽 Enter 送出與 Shift+Enter 換行
textarea.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault();
    this.send();
    textarea.style.height = 'auto'; // 重設高度
  }
});
```

---

### 3. 美化思考與推理展示 (`wukong-chat.js` & `styles.css`)

重構原生 `<details>` 標籤，透過 CSS 與 JS 補足流暢的折疊過渡。

* **CSS 美化**:
  * 移除瀏覽器預設的三角形 (`summary::-webkit-details-marker { display: none; }`)。
  * 使用自定義的 💭 符號與旋轉 icon，增加展開時的微動畫。
  * 推理區塊使用左側金黃色實線（`border-left: 2px solid var(--accent-gold)`）進行區隔，並使用柔和的背景色。

```css
.thinking {
  margin: 0.5rem 0;
  border-radius: 8px;
  background: rgba(234, 179, 8, 0.05);
  border: 1px solid rgba(234, 179, 8, 0.1);
  overflow: hidden;
}
.thinking summary {
  padding: 0.5rem 0.75rem;
  font-weight: 600;
  color: var(--accent-gold);
  list-style: none;
  display: flex;
  align-items: center;
  gap: 0.5rem;
  user-select: none;
}
.thinking summary::before {
  content: '▶';
  font-size: 0.8rem;
  transition: transform 0.2s ease;
}
.thinking[open] summary::before {
  transform: rotate(90deg);
}
.thinking .reasoning {
  border-left: 2px solid var(--accent-gold);
  margin: 0 0.75rem 0.75rem;
  padding-left: 0.75rem;
  color: var(--text-secondary);
}
```

---

### 4. 程式碼區塊快捷工具與高亮 (`wukong-chat.js` & `wukong-render`)

為 AI 生成的程式碼（Code Block）增強可讀性與實用性。

* **程式碼一鍵複製 (Copy Code Button)**:
  * 當訊息被渲染至 UI 時，透過 DOM 巡檢找出所有的 `<pre>` 元素，並在其右上角動態插入一個一鍵複製按鈕。

```javascript
// 在插入 assistant 氣泡後執行此優化
enhanceCodeBlocks(container) {
  const pres = container.querySelectorAll('pre');
  pres.forEach(pre => {
    // 建立 wrapper 與複製按鈕
    pre.style.position = 'relative';
    const button = document.createElement('button');
    button.className = 'copy-code-btn';
    button.textContent = '複製';
    
    button.addEventListener('click', async () => {
      const codeText = pre.querySelector('code')?.textContent || pre.textContent;
      await navigator.clipboard.writeText(codeText);
      button.textContent = '已複製！';
      setTimeout(() => button.textContent = '複製', 2000);
    });
    pre.appendChild(button);
  });
}
```

* **CSS 程式碼高亮風格**:
  * 雖然不引入大型高亮庫以保持 zero-build 特性，但可以透過精細的 CSS 對程式碼區塊中的結構進行基本的美化（深黑背景、優雅的圓角、細緻的系統等寬字體與微小的發光邊框）。

---

### 5. 滾動過渡與載入骨架屏 (Skeleton Screens)

為了解決點擊「跳到日期」或載入較舊訊息時，聊天歷史瞬間跳動的不適感，設計平滑載入動畫：

* **無限滾動的 Skeleton**:
  當 `loadOlder()` 觸發時，不顯示生硬的 "載入中..."，而是 prepend 一組佔位骨架（Skeleton Bubbles），具有左右搖擺的呼吸漸變：

```css
@keyframes skeleton-glow {
  0% { background-position: 100% 50%; }
  100% { background-position: 0% 50%; }
}
.skeleton {
  background: linear-gradient(90deg, #1f2937 25%, #374151 50%, #1f2937 75%);
  background-size: 200% 100%;
  animation: skeleton-glow 1.5s infinite;
  height: 2.5rem;
  border-radius: 8px;
  margin-bottom: 0.5rem;
}
```

* **平滑滾動 (Smooth Scroll)**:
  * 引入 `scroll-behavior: smooth;`。
  * 訊息送出或接收新訊息時，使用 `scrollIntoView({ behavior: 'smooth', block: 'end' })` 平滑推至底部。

---

## 測試策略

1. **視覺樣式驗證**:
   * 確認在 Light 模式與 Dark 模式下，大聖金紅色彩系統的對比度與易讀性（特別是黃色 `--accent-gold` 在淺色背景下的能見度，需特別處理 dark/light mode 配色切換）。
2. **多行 Composer 行為測試**:
   * 貼上含有換行符的長篇程式碼，驗證 textarea 確實會自動撐高至最大 `200px`，且超出後會產生內部滾動，而不損壞底部 Composer 的外觀排版。
   * 驗證按壓 `Enter` 正常送出，`Shift + Enter` 正常換行。
3. **程式碼複製測試**:
   * 驗證點擊「複製」按鈕後，剪貼簿確實能獲取無 HTML 標記的純文字程式碼，且按鈕文字會在 2 秒後復原為「複製」。
4. **細節元素相容性**:
   * 驗證 `<details class="thinking">` 在 SSE 流式推理寫入 `reasoning` 時，正在寫入的 textNode 能正常在折疊區中實時顯示。

---

## 非目標 (YAGNI)

* **引進任何第三方 JS/CSS 函式庫或外部網路依賴**: 本次體驗提升**絕對不引入任何第三方套件或外部網路服務**。舉凡程式碼高亮庫（如 Highlight.js, Prism.js）、字型庫（如 Google Fonts）、圖示庫（所有 icon 與按鈕圖標均以原生 Inline SVG 內嵌實作）均不允許引入。這能確保 Wukong 在完全無網路的離線環境下依然 100% 完美執行，同時維持極佳的 Binary 體積。
* **重度動畫庫 (如 Framer Motion / Lottie)**: 為了確保 zero-build 與最佳加載速度，只使用原生的 CSS `keyframes` 與 `transitions`。
* **自適應配色換膚管理**: 目前僅需要設計出一套符合「悟空大聖」的高質感明暗主題，不需要提供多種主題切換。

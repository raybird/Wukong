# Issue 7 交付驗證

驗證日期：2026-10-04。核准規格：`335f987:docs/issues/issue-0007/README.md`。被測內容為 `issue-0007-cli-first` 上該規格提交後的交付程式與測試；PR 的固定 BASE／HEAD 指向包含本紀錄的提交。2026-10-03 的純 CLI 基線／紅綠證據另見 [implementation-plan.md](implementation-plan.md) Phase 1。

## 最終檢查

| 命令／入口 | 實際結果 |
|---|---|
| `cargo test` | exit 0；50 suites，618 passed、0 failed、9 ignored；ignored 的 9 個真實 fixture 測試由下列探針明確執行 |
| `cargo clippy --all-targets -- -D warnings` | exit 0，無警告 |
| `cargo fmt --all -- --check` | exit 0 |
| `rustfmt --edition 2021 --check scripts/test-support/managed_actual.rs` | exit 0 |
| `bash scripts/test-docker-runtime.sh` | exit 0；37 個 `.env.example` 的 WUKONG 變數可到達容器；persistence checks passed |
| `bash scripts/test-installer-upgrade.sh all` | exit 0；installer upgrade checks passed (all) |
| `bash -n scripts/install.sh scripts/test-installer-upgrade.sh scripts/test-docker-runtime.sh` | exit 0 |
| `git diff --check` | exit 0 |

`cargo test` 的 ignored 不是通過證據；真正執行結果在下一節。以上全量程式檢查在最終 production code（含 ephemeral session ID 修補）取得；最後增加的跨回合歷史斷言以真實探針執行，未改 production code。

## 真實 OpenCode 四入口

```bash
python3 docs/issues/issue-0007/probe-managed.py --gateway-tests --all-entrances
```

OpenCode `1.18.31`，隔離 XDG／workspace／SQLite，使用本機 OpenAI-compatible provider fixture。exit 0：CLI 1、Gateway 5、Scheduler 1、Telegram 1、Web 1，共 9 個 ignored tests 實際執行成功。

- CLI 真正 binary：單選、自訂文字、`--no-stream`、`/cancel`、EOF、多問題／多選與空白答案重新詢問；多選工具結果包含 `A, B`，第二題含自訂補充。
- Gateway：回答／取消／自訂答案後跨程序續接；新 session 的 provider 請求只有本回合，續接後 provider 實際看到多回合 user message，回 `HISTORY_PRESENT` 對照，不只比較 session ID。兩個並行回合共用資料目錄、session 不同，交叉 question ID 的回覆被拒絕，正確 A／B 各自完成。
- 原生 summarize、delete、無 callback 的權限拒絕、drop 待答 future 後拒絕舊 reply；ephemeral 的 plain／streaming 回合不暴露已刪除 session ID。
- Web：真實 Axum HTTP／SSE 先收到 question，再 POST reply，回覆包含實際工具答案並收到 done。
- Telegram：dispatch 收到真實 question，使用既有 callback handler 回答，mock transport 收到最終工具答案，待答項目移除。
- Scheduler：Reject 兩回合無 marker 檔案；AllowOnce 兩回合每次各出現新的權限要求，marker 內容為 `MANAGED_PERMISSION_OK`。兩種策略下的一般 question 都以 `que_...` 路由自動拒絕。
- 程序判準從 launcher PID 追蹤子程序樹並讀取其 socket inode；執行時有程序與 HTTP／SSE 回應，完成後 `/proc` 路徑消失且所有觀察的控制埠拒絕 TCP 連線。並行測試觀察到兩個不同埠，證明沒有只檢查固定 4096。

最終探針 stdout 的 JSON 保存於下方「結果摘錄」。標準 `OPENCODE_SERVER_PASSWORD` 設為 fixture 值，本機程序正確清除它；Wukong 的 server 認證沿用既有 adapter，本次未另測該認證組合。

## Docker 真實 binary 驗證

主機 glibc binary 與 bookworm 不相容（`GLIBC_2.39 not found`），不是產品功能紅燈。使用已下載至 `/tmp/issue7-musl` 的 musl 工具及 Rust musl target，依出貨 target 建置：

```bash
CC_x86_64_unknown_linux_musl=/tmp/issue7-musl/musl-gcc \
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=/tmp/issue7-musl/musl-gcc \
cargo build --target x86_64-unknown-linux-musl -p wukong-cli

docker run --rm --init --network none --cpus 1.5 --memory 2g --pids-limit 256 \
  -v "$PWD/target/x86_64-unknown-linux-musl/debug/wukong:/usr/local/bin/wukong:ro" \
  -v "$PWD/docs/issues/issue-0007/probe-managed.py:/tmp/probe-managed.py:ro" \
  ghcr.io/raybird/wukong:v0.21.11 \
  python3 /tmp/probe-managed.py --binary /usr/local/bin/wukong
```

兩者 exit 0。runtime image 的 OpenCode 是 `1.18.29`；掛入本次新 binary，未使用映像裡的舊 Wukong 作功能證據。binary SHA256：`872d7a338f246e06c7ce3caa684db8571d064ef6828b88afed18e86f3b018119`。

- 沒有對外網路、既有 volume 或 host workspace 掛載；用映像原 entrypoint 初始化後以非 root 使用者執行。
- 5 個一次性 CLI 問答回合維持相同 session，2 個 REPL 回合各讀到正確答案；取消／EOF 完成且有回覆。
- 保持 stdin 開啟而不回答，4 秒 deadline 後 CLI 非零退出、含「逾時」，總等待受 12 秒上限約束；不因 stdin reader 延遲關閉。
- 明確 URL 指向獨立共用 server，agent command 改成不可執行的命令，2 回合仍成功收到 `SHARED_SERVER_OK`；共用 server 在兩回合之間保持存活，由探針最後停止。
- 最後追蹤的 19 個程序全部消失，唯一觀察到的控制埠 4096 已關閉。`--port 0` 可能重用可用的 4096，不宣稱每次必定選不同埠。

## Compose 與升級

以 `docker compose config --format json` 展開開發 Compose、`--profile server` 及 Memoria overlay，實際結果：預設服務只有 Web／Telegram／Scheduler；URL 空值；init 啟用；server 只在 profile 開啟時存在。overlay 各入口都有 `/opt/memoria`／`/memoria` 掛載與 `service_completed_successfully` 的 runtime 相依。release 模板由靜態 YAML 檢查核對相同預設。

在隔離 project `issue7-profile-probe` 以兩個 Alpine sleep 容器驗證：先 `--profile server up -d`，再不帶 profile `up -d --remove-orphans`，server 的實際 State 仍為 running。探針最後 `--profile server down` 清除兩容器與 network，未操作使用者部署。

installer 紅燈：加入預設部署必須停止 server 的命令斷言後，`bash scripts/test-installer-upgrade.sh docker` exit 1，missing `docker compose -p wukong --profile server stop opencode-server`。修補同 project 的啟用 helper 後 exit 0；active profile 的對照不呼叫 stop。fixture archive 現在包含真正的可選 server 定義，mock `config --services` 依該定義回答；全量 installer regression 通過。

## 問答紅綠與判準修正

- 本機 backend 實作前，真實 Gateway 回合未等待問題即結束；入口加問答 handler 前，CLI 顯示無法回答並在 8 秒 deadline 非零退出。修補後同入口真實問題／reply 完成。
- 官方本機 question endpoint 使用 `/question/{id}/reply|reject`。實測舊式 session/question 路由回 404；本機路徑改用官方 endpoint，顯式遠端 adapter 的既有路由維持。
- ephemeral 紅燈：`python3 docs/issues/issue-0007/probe-managed.py --gateway-tests` exit 1，3 passed／1 failed，`deleted helper session must not be exposed`。刪除後將 response session ID 清空，最終 plain／streaming 同組全綠。
- 本機啟動中的 future 取消、無效 loopback listener、health failure 三項 Linux 單元測試以 PID 斷言收尾，已隨全量通過；一般純 CLI 的 EOF／drop 紅綠見 Phase 1。
- 最初 fixture 的 Unicode JSON 比對與 recall 舊 prompt 造成誤測，已改為當前 user input；不作產品紅燈。`0` 在允許自訂答案的 OpenCode question 是合法自訂文字，故無效答案對照改成空白；多選／第二題的實際工具答案另有斷言。
- 刪除 session 後 adapter 會重新建立新 session，測試因此斷言不再續接已刪除 ID；未把既有重建行為當成產品錯誤。

## 限制與精煉

模型輸出可控制，沒有真實外部 LLM 請求。Web 測真實 router／SSE，未做瀏覽器 UI；Telegram transport 為 mock，未發送外部訊息；Scheduler 測 executor，未等待 cron。Docker 真實測新 CLI binary 與原 runtime image，沒有完整 release image 重建、四個常駐入口整套部署或長期壓力測試。Linux 及 Docker 的程序回收已測，其他作業系統與工具自行 daemonize 不在證據內。

顯式 server 的文字回合／選擇已測；其既有一般 question 路由未修改，不以本機問答證據宣稱最新 OpenCode 的遠端一般 question 相容。`.env` 自訂值保留，已有額外 run 旗標或 server URL 的部署需按 [docker.md](../../docker.md) 遷移。保留期刪除與 vacuum 僅在顯式專用 server 生效。

2026-10-04 依 code-simplify 檢查本次變更：共用既有 server adapter，生命週期、compact／delete 共用同一收尾路徑，CLI 合併 stream／non-stream 問答；必要 guards 分別負責 child、stdout drain 與待答路由。未建立 provider framework 或改記憶 schema；沒有可證明安全且更清楚的額外重構，精煉 no-op。

## 提交前影響範圍（2026-10-04）

`npx gitnexus analyze --force --skip-agents-md` 成功重建（6,242 nodes／12,783 edges）；未建立 CLAUDE.md。`gitnexus_detect_changes(scope=staged)`：29 檔、163 個 changed symbols、24 個 affected flows、CRITICAL。變更檔案均為預期 CLI／Gateway／四入口測試／Compose／installer／文件；沒有修改前端 JS 或記憶 schema。同名 reply／main 的跨語言推斷使部分流程為過度近似，不能把圖的廣度當成無風險。實際 readiness、問答與 session 路徑由四入口及全量測試保障，完整固定 diff 另交獨立 reviewer 核對。

修改前的 `build_backend_from_env` HIGH 與 renderer `on_event` CRITICAL 已告知；新增尚未索引的 Local symbol 與 shell function 回 UNKNOWN 時另讀直接呼叫點與相關回歸，不把 UNKNOWN 記為 LOW。

## 結果摘錄

```text
{"root": "/tmp/issue7-managed-2x6iy1uw", "launched": 44, "remaining": 0, "closed_ports": 2, "tracked_processes": 44, "ports": [4096, 43415]}
{"root": "/tmp/issue7-managed-the18kem", "opencode": "1.18.29", "session": "ses_efd24b8cfffeJAxVXKnYb37Qbt", "cli_rounds": 5, "repl_rounds": 2, "timeout": true, "shared_server_rounds": 2, "launched": 19, "remaining": 0, "closed_ports": 1, "tracked_processes": 19, "ports": [4096]}
ok: all 37 WUKONG_* variables offered in .env.example reach a container
docker runtime persistence checks passed
installer upgrade checks passed (all)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.27s
```

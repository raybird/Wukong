# RC Rehearsal

Run every RC in isolated Binary and Docker directories before stable promotion:

```bash
./scripts/rehearse-rc.sh --from v0.17.1 --to v0.18.0-rc.1 --binary-home /tmp/wukong-rc-binary --docker-dir /tmp/wukong-rc-docker --evidence docs/release-rehearsals/v0.18.0-rc.1.json
```

Provide controlled Telegram, Scheduler, and credential checks through `WUKONG_REHEARSAL_*_CHECK`. Snapshot SQLite consistently before each row. Evidence records hashes and statuses only, never secret values. Retain the committed report. Stop on a failed health, compatibility, rollback, or state-preservation check; preserve transaction backups for recovery. A stable promotion requires this PASS report.

## OpenCode 版本

發版映像檔的 opencode 版本**由 CI 在發版當下解析成 npm 上的最新版**，不是用 repo 裡 `release/runtime-inputs.env` 的值。`scripts/resolve-opencode-version.sh` 會同時改寫版本、npm integrity 與 `release/package-lock.json`（`Dockerfile.release` 走 `npm ci`，三者不一致會直接建置失敗），所以「取最新版」不等於拿掉釘版——最終映像檔仍是確切版本 + integrity，`release-manifest.json` 也記錄同一個版本。

因此：

- **repo 裡的 `OPENCODE_VERSION` 只是本機建置與測試的預設值**，不代表下次發版會用的版本。要知道實際版本，看 release manifest 或該次 workflow 的 summary。
- **RC 驗收要涵蓋 opencode 換版的風險**。版本在 CI 才決定，所以 RC 與 stable 之間若隔了一段時間，兩者的 opencode 可能不同版。要讓 stable 用與 RC 相同的版本，先設下面的 pin。
- **卡版（escape hatch）**：在 GitHub repository variables 設 `OPENCODE_VERSION_PIN=X.Y.Z`，workflow 會改用該版本而不取最新。opencode 出回歸、或想讓 stable 沿用 RC 驗過的版本時使用；解除就是刪掉該變數。

本機要手動同步 repo 的釘版（例如讓本機建置跟上），直接跑：

```bash
bash scripts/resolve-opencode-version.sh          # 取最新版
OPENCODE_VERSION_PIN=1.18.14 bash scripts/resolve-opencode-version.sh   # 指定版本
```

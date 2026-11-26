# RC Rehearsal

Run every RC in isolated Binary and Docker directories before stable promotion:

```bash
./scripts/rehearse-rc.sh --from v0.17.1 --to v0.18.0-rc.1 --binary-home /tmp/wukong-rc-binary --docker-dir /tmp/wukong-rc-docker --evidence docs/release-rehearsals/v0.18.0-rc.1.json
```

Provide controlled Telegram, Scheduler, and credential checks through `WUKONG_REHEARSAL_*_CHECK`. Snapshot SQLite consistently before each row. Evidence records hashes and statuses only, never secret values. Retain the committed report. Stop on a failed health, compatibility, rollback, or state-preservation check; preserve transaction backups for recovery. A stable promotion requires this PASS report.

## OpenCode 版本

發版映像檔的 OpenCode 是 **repo 裡 `release/runtime-inputs.env` 釘選的 V2 版本**（`@opencode/cli`，2026-11-25 起為 2.3.2，issue-0044）。npm 上有更新版時不會跟著升：Wukong 對 V2 的協定有自己的 adapter，換版前要先驗過。`scripts/resolve-opencode-version.sh` 在 CI 重新向 npm 取該版的 integrity，連同 `release/package.json`、`release/package-lock.json` 一起寫回（`Dockerfile.release` 走 `npm ci`，三者不一致會直接建置失敗）；`release-manifest.json` 記錄同一個版本，`scripts/test-release-image.sh smoke` 在發布 tag 前直接問映像內的 `opencode --version`，與釘選不同就中止。

- **換版**：改 `release/runtime-inputs.env` 的版本，跑下面的命令重產三個檔，再跑 issue-0044 的驗證（`scripts/test-opencode-baseline.sh` 等）後提交。
- **臨時改用其他版本**：在 GitHub repository variables 設 `OPENCODE_VERSION_PIN=X.Y.Z`，只接受 2.x；V1（1.x）會被拒絕。解除就是刪掉該變數。

```bash
bash scripts/resolve-opencode-version.sh                                # 依 runtime-inputs.env 的版本重產
OPENCODE_VERSION_PIN=2.3.2 bash scripts/resolve-opencode-version.sh     # 指定版本（只收 2.x）
```

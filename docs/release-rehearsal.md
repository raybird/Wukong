# RC Rehearsal

Run every RC in isolated Binary and Docker directories before stable promotion:

```bash
./scripts/rehearse-rc.sh --from v0.17.1 --to v0.18.0-rc.1 --binary-home /tmp/wukong-rc-binary --docker-dir /tmp/wukong-rc-docker --evidence docs/release-rehearsals/v0.18.0-rc.1.json
```

Provide controlled Telegram, Scheduler, and credential checks through `WUKONG_REHEARSAL_*_CHECK`. Snapshot SQLite consistently before each row. Evidence records hashes and statuses only, never secret values. Retain the committed report. Stop on a failed health, compatibility, rollback, or state-preservation check; preserve transaction backups for recovery. A stable promotion requires this PASS report.

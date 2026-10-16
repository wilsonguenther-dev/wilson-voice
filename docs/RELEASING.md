# Releasing Yap — permission-identity check

Before publishing any build, the release seat runs `bash scripts/qa/perm-probe.sh --app /Applications/Yap.app --compare <the new Yap.app>` and, when it prints `dr_change=yes`, copies its `warning:` line verbatim into the release notes (every Accessibility, Input Monitoring and Full Disk Access grant for the old build stops matching — the 0.9.0 failure). The full runbook is [RELEASE.md](RELEASE.md).

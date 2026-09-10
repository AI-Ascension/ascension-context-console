# Workflows

Changes use a feature branch and pull request. CI runs the locked Rust policy, format, Clippy and
tests with read-only contents permission and bounded timeouts. No workflow accesses provider/game
credentials or proprietary files.

Repository creation, issue and draft PR publication are separate from merge, release, deployment,
service installation and game/provider execution. Each evidence record names its exact commit and
scope.

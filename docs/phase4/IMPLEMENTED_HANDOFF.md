# Phase 4 implementation handoff (fixture/source scope)

Date: 2026-09-11. The current target baseline is `f6b4284b5cb98a9900373a2e9783704e3ce5e28f`.
The companion harness baseline is `99168da54c3b09b2cfb08b542ef8b014bd0bf5af`.

| Seam | Owner and implementation | Evidence | Limit |
| --- | --- | --- | --- |
| Frozen capture, renderer and Phase 2 held boundary | companion harness `context_capture`, `context_control`, `context_memory` and Exo modules | inherited Phase 1–3 tests and handoff | live provider/game remains unverified |
| Persistent-session policy and identities | harness `provider_session::{types/*,broker*}` | `tests/provider_session.rs` | in-memory journal; native binary compatibility unverified |
| Bounded native framing | harness `provider_session::{protocol*,transport*}` | compiled `provider-session-peer` test | fixture envelope is not proof of Codex App Server behavior |
| Client API and CLI | target `provider_session.rs`, `run_phase4_cli`, additive OpenAPI seed | `tests/phase4_session.rs`; `phase4-cli` | target owns no native state or credentials |

The native three-level Luna/max preflight was attempted twice and both streams disconnected before
metadata could be decoded. No delegated implementation claim is made. No real provider, game,
deployment, or GitHub publication was performed. The fixture lane is intentionally `fixture_only`;
native-binary/fake-upstream and live-provider capabilities remain `unverified`.

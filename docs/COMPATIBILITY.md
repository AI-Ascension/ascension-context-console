# Compatibility

Phase 1 supports Linux source builds with Rust 1.97.1 and offline synthetic fixtures. The reader
and store are platform-neutral Rust components. The checked-in browser evidence was captured on
Linux with Chromium `153.0.8010.12` at `1440x1000` and `375x800`, with reduced motion enabled.
The browser checks cover the synthetic same-origin bundle in that environment.

The integrated browser audit uses the same Linux/Chromium environment and a loopback Rust demo
server. It does not claim cross-platform browser or native storage coverage.

Provider, game-host, Windows, deployment and external observability compatibility is unverified.
The console does not launch a provider or game and cannot repair adapter differences. Map/image
capture is unsupported until an accepted producer boundary supplies it.

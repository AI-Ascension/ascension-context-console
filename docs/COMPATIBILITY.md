# Compatibility

Phase 1 supports Linux source builds with Rust 1.97.1 and offline synthetic fixtures. The reader
and store are platform-neutral Rust components. Browser checks are limited to standards-compliant
modern browsers in the tested local environment.

Provider, game-host, Windows, deployment and external observability compatibility is unverified.
The console does not launch a provider or game and cannot repair adapter differences. Map/image
capture is unsupported until an accepted producer boundary supplies it.

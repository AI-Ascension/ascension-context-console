# Changelog

## Unreleased

- Read the `Check documentation links` step's verdict from the log instead of `cargo`'s exit code.
  A renamed, removed or mistyped `rustdoc::` deny is not a hard failure: rustdoc reports an inert
  `warning[E0602]: unknown lint` and exits 0, so the step could pass while denying a lint it still
  names. The step now tees its output and fails on any `error`, `warning` or `unknown lint` line,
  printing an `exit/errors/warnings/unknown_lints` counter line so a renamed deny is distinguishable
  from a real link defect. Measured on the unmodified tree: the hardened step exits 0 with
  `doc: exit=0 errors=0 warnings=0 unknown_lints=0`, exits 1 with `unknown_lints=1` when one deny is
  renamed, and still exits 1 with `errors=2 unknown_lints=0` on a genuine unresolved link. No source
  file changed.

- Denied `rustdoc::private_intra_doc_links` and `rustdoc::redundant_explicit_links` in the new
  `Check documentation links` step, alongside `broken_intra_doc_links`. Both are warn-by-default, so
  without the deny a link from a public item to a private one prints a warning that resolves only
  because the gate always passes `--document-private-items` — a class the step can never observe on
  any run. No link is broken at this revision and no source file changed; the three-lint command
  exits 0 with zero warnings on the unmodified tree and exits 101 when a public-to-private link is
  added, where the single-lint command exits 0 with one warning.

- Validate v3 capability publications and supplied effective-limit records at the scoped owner
  route, enforce disclosed memory query limits before delegation, and retain explicit v1 rollback.

- Bootstrapped the private Phase 1 read-only Context Console target.

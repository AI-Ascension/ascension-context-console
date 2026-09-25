# Changelog

## Unreleased

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

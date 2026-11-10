# Upstream integration checks

`audit_launcher_restoration.py --upstream <sha> --upstream-base <sha>` inventories
changed launcher files and fork commits. It marks whole upstream takes without
judging whether a fork hunk is a permitted Horizon-layer restoration or mod work
that must remain. Reviewers check the inventory and the resulting corrections.

## File-size baseline provenance

`tools/check_file_size.py --base <base>` permits additions or raised caps only
when the full SHA in `tools/file_size_baseline.txt` changes and:

- the SHA resolves to an available commit;
- it is an ancestor of upstream main, freshly fetched from
  `https://github.com/horizon-rust/horizon.git` into `refs/sync/upstream-main`;
- it is not an ancestor of the merge base;
- it is a direct parent of a merge commit in `base..HEAD`.

A fake SHA, an old base ancestor, an ordinary new ancestor without a merge,
and missing provenance all fail. Entries above the current non-test size fail
as stale-high, including entries for deleted files. Other changes may only lower
or remove caps. The PR body lists every cap and its growth for human review.

A fork-only side-branch merge cannot establish upstream provenance. A PR could
still merge a real upstream commit and add unrelated caps: review must confirm
that this is an upstream sync and that the listed restoration growth is justified.

The fetch is required on every changed sync declaration; an unreachable upstream,
a timeout or missing history fails closed, even if a previous fetch left a cached
ref. Dyna verify therefore fails, with a network-required message, if its runner
cannot reach GitHub. `HORIZON_UPSTREAM_URL` overrides the URL for local bare-repo
tests; normal CI leaves it unset.

The Dyna verify checkout initially uses the checkout action's shallow default,
then its `Fetch this commit's history` step fetches `HEAD` with `--unshallow` before
running the file-size check. This retains the upstream-parent merge in history.
The checker refuses a still-shallow checkout with an explicit full-history error;
it never treats missing history as approval.

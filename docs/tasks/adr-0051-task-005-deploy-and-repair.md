# ADR 0051 Task 005: Deploy And Repair

Owner: [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md)
section 6. Plan: [phase plan](../plans/adr-0051-source-url-phase-plan.md).

**The operator does this task on the VPS.** It is not for a coding model.
Tasks 001 to 004 must be merged and their gates green.

## Goal

The node on the VPS runs ADR 0051. One forced pass restores the content and
the payment routes of each damaged record from its source URL.

## Steps

1. Read the `VERIFIER_CHAIN` value of the primary on the VPS. If it names
   `crawl_token`, remove the name before the deploy. If the value is unset,
   the new default applies. After task 001, a value with `crawl_token` stops
   the primary at startup.
2. Make sure that `CRAWL_TOKEN` on the primary is not empty.
3. Make sure that no crawler pass runs.
4. Make a consistent backup of the primary database.
5. Deploy the node image, then the crawler image.
6. Make sure that the primary answers `GET /health`, and that a normal
   crawl of one known feed gives `accepted` or `no_change`.
7. Record the candidate list before the pass. A candidate is a feed with an
   observation at a URL that is not its source URL:

   ```sql
   SELECT f.feed_guid, f.feed_url, o.url
   FROM feeds f JOIN feed_url_observations o ON o.feed_guid = f.feed_guid
   WHERE o.url <> f.feed_url;
   ```

   Also record the title and the payment routes of each candidate.
8. Apply each source URL body again with `force_reingest`. When a recent
   `refresh` pass kept its bodies, replay its fetch cache with
   [task 006](adr-0051-task-006-replay-fetch-cache.md). That sends no request
   to a feed host. Otherwise run `refresh --force`.
9. Record the title and the payment routes of each candidate again. Write
   each changed record in a review record in `docs/reviews/`.
10. For a sample of the changed records, compare the routes on each community
    node with the routes on the primary.

## Acceptance Criteria

Mechanical:

- Step 6 gives `accepted` or `no_change`.
- For a sample of candidates, a fetch of the source URL gives the routes that
  the API reports after the pass.

Manual. A person must do these. If one cannot run, report it as open:

- The operator reads the list of changed records and decides if a publisher
  needs a contact.
- The operator records the result in `AGENTS.md` under "Where The Work
  Stands", in the same commit as the review record.

## Rollback

Deploy the previous node image. The repaired rows stay. They are correct
content from the source URLs. The previous binary has the takeover defect
again.

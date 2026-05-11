# AI rate limit reached

The Claude driver returned a rate-limit notice instead of completing the dispatch. Continuing to retry burns more budget without making progress, so the orchestrator is halted until you remove this file.

**Triggering response:** `.//home/ace/Desktop/prjx/kitchensync/reports/2026-05-11-08-41-04-498-6d52c8_subpjx-gitignore-matcher_WriteGlue_RESPONSE.md`

Resolve by waiting for the limit window to reset (check the triggering response for the reset time), then delete this file and restart the orchestrator.

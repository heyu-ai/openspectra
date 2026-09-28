Archive sub-flow for commit step 6a. The parent supplies the selected target and reviewed commit plan. Preview before archive authorization; return to step 7.

6a. **Archive sub-flow** (only when the user selected "Archive first, then commit together")

    **6a-i. Cache pre-move metadata and status**

    Cache before archive:

    - proposal summary used by the commit message
    - task progress and incomplete-task list
    - complete touched source-file mapping, including each entry's provenance or unverified state
    - active artifact paths under `{{SPEC_DIR}}changes/<name>/`
    - pre-archive Git status from `git status --porcelain=v1 -z`, parsed as NUL records with both rename paths

    Run `spectra status --change "<name>" --json`; retain incomplete artifact status and current-session verification evidence, including known CRITICAL findings and locations. Keep the original commit plan and cached tracking data unchanged until archive succeeds.

    **6a-ii. Preview the archive transaction**

    Run exactly one read-only preview:

    ```bash
    spectra archive <name> --preview --json
    ```

    Display the preview's incomplete tasks, delta application plan, conflicts, and preview warnings together. A blocking parse or validation error ends the sub-flow: show the error, do not execute archive, and do not stage files.

    **Confirm the complete archive plan**

    Show incomplete artifacts and tasks, delta actions, conflicts, preview warnings, known CRITICAL findings with locations, cleanup implications and selected flags together. Recommend verify/review when current-session evidence is absent; warnings remain advisory.

    Preserve incomplete tasks unless `--mark-tasks-complete` is explicitly chosen. Apply delta specs once by default; explicit skipping uses `--skip-specs`. Omit the delta choice when no specs exist.

    If the unchanged complete plan is already explicitly authorized, proceed. Otherwise collect missing choices and approval together; the archive-first selection alone cannot authorize undisclosed flags or warnings. Silence or elapsed time supplies no missing choices. A changed plan invalidates affected approval: refresh and present changes before execution. If the user cancels, stop without mutation.

    **6a-iii. Archive execution and file collection**

    Start from this command, insert confirmed `--mark-tasks-complete` and/or `--skip-specs` before `--json`, and execute the constructed core archive command exactly once:

    ```bash
    spectra archive <name> --json
    ```

    Execution failure: report and STOP; must not stage files. Preserve the prepared message, original commit plan and touched tracking data.

    After success, parse `archived_id`, the actual `archived_path`, and `cleanup_warnings` from the structured result. Use the returned path exclusively. Re-run `git status --porcelain=v1 -z` and compare it with the pre-archive Git status cache. Build the archive additions only from the returned `archived_path`, active deletions from cached active artifact paths, and spec or other archive-produced changes from the status delta. Exclude concurrent unrelated changes; status deltas alone are not archive provenance.

    Display an **updated commit plan** with: Active Artifact Deletions; Archived Additions from the returned `archived_path`; verified Source Files from the cache; Unverified Legacy Tracking excluded unless explicitly confirmed; Spec Changes and other archive-produced files from the status delta; and every `cleanup_warnings` entry.

    Capture a fresh scope snapshot and complete candidate content for the rebuilt plan; retain the cached source provenance, shared-file exclusions and unverified legacy state. Include both `path` and `old_path` for scope renames. Compare full content and message with the approved plan; rebuild the archive-aware message using cached metadata and the parent conventions. Return the new snapshot exclusively to the parent's native path-limited commit flow, including conditional new-file registration and index protection.

    Show the full content and message with new files or warnings. If `cleanup_warnings` is non-empty, explain that archive succeeded but cleanup was incomplete. Changed content, message or warnings require one updated-plan approval before staging; an unchanged complete plan retains its approval. Keep cleanup and message choices in this single approval.

    Cancellation now means **archived, not committed**: report `archived_id`, `archived_path` and any pending cleanup warnings. Preserve the completed archive and do not repeat archive or roll it back. A later commit failure leaves the same archive result intact; if a commit was created, the parent reports its actual hash and pending issue separately.

    From this point onward, do not read the moved active proposal or tasks. Continue with the returned `archived_id` and `archived_path`, cached proposal summary, cached task progress, cached touched mapping, and post-archive status only.


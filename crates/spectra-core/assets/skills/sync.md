The standalone manual sync workflow is retired. This compatibility lookup only explains the migration; it performs no mutation and does not authorize or automatically invoke archive.

Core archive owns delta-spec application. With explicit user intent to archive a completed change, use `spectra archive <name> --preview --json` to prepare the reviewable plan, then the archive workflow for authorized execution. Archive applies specs and moves the change in one transaction. A request for this lookup alone is not archive intent.


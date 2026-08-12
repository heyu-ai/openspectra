# Search specification

## ADDED Requirements

### Requirement: Search interface

The system SHALL accept `spectra search <QUERY>` with `--limit`, `--json`, and
the global `--no-color` option. The default limit SHALL be 10.

#### Scenario: Search an initialized project

- **WHEN** a user searches an initialized project containing matching
  Markdown artifacts
- **THEN** the command exits successfully and returns no more than the
  requested number of results in descending relevance order

#### Scenario: Search outside a project

- **WHEN** a user searches outside an initialized project
- **THEN** the command exits non-zero and reports that initialization is
  required on stderr

### Requirement: Offline lexical retrieval

The system SHALL rank Markdown files below the configured spec directory with
dependency-free BM25-style lexical retrieval and SHALL NOT require a network
request, model, or persistent index.

#### Scenario: Search CJK content

- **WHEN** a CJK phrase (Han ideograph, Japanese kana, or Korean Hangul) occurs
  in a document
- **THEN** unigram and bigram tokens make that document eligible for a match,
  so a shorter unspaced query matches a longer unspaced phrase

#### Scenario: Empty corpus

- **WHEN** the spec directory contains no Markdown files
- **THEN** search exits zero and returns no results

### Requirement: Project-root containment

The system SHALL only read and surface Markdown files whose real, symlink-
resolved path is inside the project root. A `spec_dir` that is absolute or
contains `..` SHALL be rejected. A `spec_dir` that is itself a symlink SHALL be
refused unless it resolves inside the project root. Symlinked entries
encountered while walking the tree SHALL be skipped (not followed), so a
symlink can never surface a file outside the project root.

#### Scenario: Symlinked spec directory escaping the project root

- **WHEN** the spec directory is a symlink that resolves outside the project
  root
- **THEN** search refuses it and returns no results from outside the root

#### Scenario: Symlinked file within the tree

- **WHEN** a Markdown entry within the tree is a symlink
- **THEN** search skips it rather than following it, so its target is never
  read or surfaced

### Requirement: JSON consumer contract

JSON output SHALL contain `query` and `results`; every result SHALL contain a
project-relative `path`, a numeric normalized `score`, and a `snippets` array.
Successful OpenSpectra search output SHALL omit the oracle's index/model
`error` field because OpenSpectra has no such prerequisite.

#### Scenario: Structured result

- **WHEN** `--json` is supplied and a document matches
- **THEN** the response is compact JSON with the documented fields and a
  trailing newline

### Requirement: Deterministic boundaries

The system SHALL accept a zero limit as a successful empty result and SHALL
reject values that cannot fit the platform `usize` through CLI parsing.

#### Scenario: Zero result limit

- **WHEN** `--limit 0` is supplied
- **THEN** the command exits zero and returns no results

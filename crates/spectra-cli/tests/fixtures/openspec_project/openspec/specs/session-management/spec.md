# Session Management Specification

## Purpose

Session management keeps customer accounts secure while preserving a predictable
sign-in experience.

## Requirements

### Requirement: Session Expiration

The system MUST expire sessions after 30 minutes of inactivity.

#### Scenario: Idle timeout

- **WHEN** 30 minutes pass without user activity
- **THEN** the session is invalidated

### Requirement: Remember Me

The system MUST allow users to remain signed in for 30 days.

#### Scenario: Persistent login

- **WHEN** a user selects remember me
- **THEN** the session remains valid across browser restarts

### Requirement: Legacy Audit Log

The system MUST record session creation in the legacy audit stream.

#### Scenario: Session starts

- **WHEN** a user signs in
- **THEN** a legacy audit event is recorded

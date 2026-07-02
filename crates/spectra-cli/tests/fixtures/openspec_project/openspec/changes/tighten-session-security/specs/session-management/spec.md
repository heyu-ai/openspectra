## ADDED Requirements

### Requirement: Device Trust

The system MUST challenge sessions created from unknown devices.

#### Scenario: Unknown device

- **WHEN** a user signs in from a new device
- **THEN** the session requires an additional verification step

## MODIFIED Requirements

### Requirement: Session Expiration

The system MUST expire sessions after 15 minutes of inactivity.
(Previously: 30 minutes)

#### Scenario: Idle timeout

- **WHEN** 15 minutes pass without user activity
- **THEN** the session is invalidated

## REMOVED Requirements

### Requirement: Remember Me

Persistent login is removed in favor of explicit reauthentication.

## RENAMED Requirements
- FROM: `### Requirement: Legacy Audit Log`
- TO: `### Requirement: Session Audit Trail`

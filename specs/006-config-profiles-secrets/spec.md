# Feature Specification: Externalized Configuration, Profiles, and Secret Management

**Feature Branch**: `006-config-profiles-secrets`

**Created**: 2026-11-08

**Status**: Draft

**Input**: User description: "externalize passwords of mongodb, remove all hardcodings from the code and MD files, provide a way start the service with profile like local and production"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Profile-Driven Operational Execution (Priority: P1)

Operators and developers need to run the metadata extraction service across distinct environments (such as `local` development and `production` operations) by selecting an operational profile via CLI or environment variable, without manually configuring individual connection parameters or toggling developer settings.

**Why this priority**: Profile selection provides the foundational runtime framework for separating environment-specific behaviors, connection topologies, log verbosity, and security enforcement policies.

**Independent Test**: Can be tested by invoking the application with `--profile local` and `--profile production`, verifying that each profile resolves its corresponding configuration defaults, validation constraints, and operational targets.

**Acceptance Scenarios**:

1. **Given** a system with no runtime arguments other than `--profile local`, **When** the service starts, **Then** it initializes using the local development profile defaults (e.g., local host bindings, default port, relaxed security checks for local testing).
2. **Given** a system with `--profile production`, **When** the service starts, **Then** it enforces strict production validation rules (e.g., requiring explicit non-default credentials, rejecting unauthenticated connection endpoints, and applying production log levels).
3. **Given** the profile environment variable `APP_PROFILE=production` set in the host environment without explicit CLI profile flags, **When** the service starts, **Then** it automatically activates the `production` profile.
4. **Given** an invalid or unrecognized profile name (e.g., `--profile staging_invalid`), **When** the service starts, **Then** it halts immediately with a clear error message indicating supported profile options.

---

### User Story 2 - Secure Credential Externalization & Secret Injection (Priority: P2)

Security administrators and operators require that database passwords and sensitive connection parameters are externalized via environment variables or secure configuration mechanisms, ensuring that credentials are never stored in plaintext within source code, configuration files committed to source control, or command history.

**Why this priority**: Eliminates security vulnerabilities and compliance risks associated with credential leakage in repositories and process argument lists.

**Independent Test**: Can be tested by setting connection secrets exclusively through environment variables (e.g., `MONGODB_PASSWORD` or `MONGODB_URI`) without passing passwords in plaintext CLI arguments, verifying successful authenticated database connection.

**Acceptance Scenarios**:

1. **Given** a database connection string with a password placeholder or omitted password and an environment variable providing the secret, **When** the service initializes the storage connection, **Then** the secret is resolved and merged at runtime into the authenticated connection without exposing the password in logs or telemetry.
2. **Given** an environment variable containing the entire connection URI, **When** the service runs without CLI database arguments, **Then** it transparently consumes the URI from the environment.
3. **Given** the service is executed in `production` profile and required database credentials are missing from the environment, **When** initialization runs, **Then** the service terminates immediately with an explicit, actionable error explaining which secret is missing.
4. **Given** CLI arguments specify credentials while environment variables also define them, **When** configuration is resolved, **Then** the system follows a deterministic precedence order (CLI explicit override > environment variables > profile configuration defaults).

---

### User Story 3 - Repository & Documentation Sanitization (Priority: P3)

Developers and technical auditors require that all documentation files, examples, code comments, and automated test fixtures are completely scrubbed of hardcoded passwords and actual connection credentials, using standardized placeholders and externalized configuration patterns instead.

**Why this priority**: Prevents inadvertent exposure of sample or production passwords in git history, documentation guides, and copy-pasted quickstart commands.

**Independent Test**: Can be verified by running a full repository text search for historical hardcoded credential strings, ensuring zero occurrences in markdown documentation, comments, and application code.

**Acceptance Scenarios**:

1. **Given** the project documentation files (including README and quickstart guides), **When** reviewed by users, **Then** all connection examples demonstrate externalized credential patterns using environment variables or placeholders (e.g., `<MONGODB_PASSWORD>`).
2. **Given** automated unit and integration tests, **When** executed in an isolated environment without pre-configured secrets, **Then** tests either use non-sensitive local defaults, mock the storage layer, or source credentials from environment variables without hardcoded passwords in test source code.
3. **Given** sample environment template files (e.g., `.env.example`), **When** inspected by developers, **Then** all secret values contain placeholder syntax without actual passwords.

---

### Edge Cases

- **Missing Profile Configuration**: What happens when a user specifies a valid profile name (e.g., `production`) but the associated environment variables or configuration file cannot be found? The system MUST halt at startup with an exit code indicating missing configuration and display which required variables are unpopulated.
- **Special Characters in Externalized Passwords**: How does the system handle externalized passwords containing URL-reserved characters (e.g., `@`, `:`, `/`, `%`)? The system MUST safely URL-encode the externalized password when interpolating into connection URIs to avoid connection string corruption.
- **Dual Secret Specifications**: What happens when both a full connection URI environment variable and a specific password environment variable are provided? The specific secret MUST take precedence or be deterministically merged into the URI's credential component.
- **Test Execution Without Database**: What happens when automated test suites run in environments where no live database is running? Tests MUST gracefully detect database absence and either run against mocked/isolated components or provide clear guidance rather than hanging or failing with uninformative panics.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST support named operational profiles, including at least `local` and `production`.
- **FR-002**: System MUST allow selecting the active profile via a command-line option (`--profile <NAME>`) and an environment variable (`APP_PROFILE` or `MEDIA_PROFILE`), with CLI arguments taking precedence over environment variables.
- **FR-003**: System MUST provide sensible defaults for the `local` profile to facilitate rapid local development and testing without requiring manual configuration.
- **FR-004**: System MUST enforce strict validation in the `production` profile, failing fast if mandatory credentials or connection endpoints are missing.
- **FR-005**: System MUST allow externalizing database passwords through environment variables (such as `MONGODB_PASSWORD` or `MONGODB_URI`).
- **FR-006**: System MUST securely interpolate externalized credentials into connection configurations without logging or printing plaintext passwords in any output, log stream, or error trace.
- **FR-007**: System MUST support template configuration or environment files (such as `.env.example` or profile definition files) showing required variables with safe placeholder values.
- **FR-008**: System MUST purge all hardcoded credentials from source code files, test files, and markdown documentation, replacing them with standard placeholders or environment variable references.
- **FR-009**: Automated integration tests MUST source credentials from external environment variables or local fallback configurations rather than hardcoded credentials in test files.
- **FR-010**: System MUST define and adhere to a strict configuration resolution hierarchy: CLI arguments override environment variables, which override profile configuration files, which override built-in defaults.

---

### Key Entities *(include if feature involves data)*

- **Runtime Profile**: Represents an operational environment mode (e.g., `local`, `production`) defining baseline default settings, connection rules, log verbosity, and credential validation strictness.
- **Application Configuration**: The unified configuration resolved at startup, encapsulating storage parameters, scanning boundaries, batching rules, and profile metadata.
- **Secret Provider**: An abstraction responsible for retrieving sensitive credentials from external sources (environment variables, credential stores, or environment files) without exposing them in memory dumps or logs.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Zero plaintext credentials exist across the entire repository (source code, configuration templates, markdown documentation, and tests), verified by automated repository audits.
- **SC-002**: Operators can switch entire application operational behavior between `local` and `production` with a single argument (`--profile`) or single environment variable, without passing any database passwords on the command line.
- **SC-003**: 100% of sensitive credentials passed via environment variables remain masked in all application log outputs, status messages, and error diagnostics.
- **SC-004**: System startup validation completes in under 100 milliseconds, failing fast with descriptive error messages when required production configuration is missing.
- **SC-005**: Existing automated test suites pass without requiring hardcoded secrets in test source code.

---

## Assumptions

- Operating environments provide standard environment variable mechanisms for injecting secrets (e.g., container runtimes, shell environments, `.env` files).
- The `local` profile assumes access to a developer workstation environment, defaulting to standard local port conventions while still supporting credential overrides.
- The `production` profile assumes a hardened environment where credentials are provided via secure environment injection or orchestration secret stores.
- Development teams have standard tooling for managing `.env` files (e.g., copying `.env.example` to `.env` during initial setup).

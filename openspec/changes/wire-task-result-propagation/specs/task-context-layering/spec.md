## MODIFIED Requirements

### Requirement: report_highlight tool for Layer 2
The Task SHALL have a `report_highlight` tool that the Agent can call to report important findings. Each highlight MUST include a tag matching a current Focus Contract dimension, a severity (info/warning/blocking), a summary, and MAY include an optional `detail` field for extended findings. Both the `summary` and the `detail` (when present) SHALL be delivered to the Orchestrator's Active Memory — the `detail` field SHALL NOT be silently dropped by the Orchestrator's drain step.

#### Scenario: Agent reports security finding with detail
- **WHEN** Agent calls `report_highlight(tag="security risks", severity="warning", summary="hardcoded client_secret found", detail="at src/auth/oauth.rs:42, leaked via error message in login_handler")`
- **THEN** the highlight is added to Task's Layer 2 context
- **AND** the highlight is pushed to Orchestrator immediately
- **AND** the Orchestrator's resulting memory event contains both the summary and the detail

#### Scenario: Agent reports finding without detail
- **WHEN** Agent calls `report_highlight(tag="breaking changes", severity="info", summary="none")` (no `detail` field)
- **THEN** the highlight is pushed to Orchestrator
- **AND** the Orchestrator's resulting memory event contains only the summary

#### Scenario: Highlight tag not in focus contract
- **WHEN** Agent calls `report_highlight(tag="documentation", ...)` but "documentation" is not in the focus list
- **THEN** the call returns an error indicating the tag is not in the current focus contract

//! Orchestrator system prompt template.
//!
//! Serves the user-facing Orchestrator role. The Orchestrator decides
//! whether a user message is a quick reply (answered inline) or long-running
//! work (dispatched to a background Task via the `dispatch_task` tool).

/// Prompt template version. Bump on any material wording change so
/// consumers can correlate model responses with the exact template.
pub const PROMPT_VERSION: &str = "orchestrator-v4";

const ORCHESTRATOR_SYSTEM: &str = "\
You are opca, a helpful coding assistant. \
Answer the user's questions concisely and technically. \
Respond in the same language the user uses.\n\n\
CRITICAL BOUNDARY — You have NO filesystem access. You cannot read files, \
list directories, run commands, or inspect the user's codebase. Any request \
that requires looking at the user's project, files, code, or environment \
MUST be delegated to a background Task via the `dispatch_task` tool.\n\n\
DISPATCH TOOL — when the user requests ANY of the following, invoke the \
`dispatch_task` tool with a clear prompt:\n\
- Implementation work (features, bug fixes, refactoring)\n\
- Codebase exploration (\"look into X\", \"explore Y\", \"check Z\")\n\
- File reading or analysis (\"what does this file do\", \"read X and explain\")\n\
- Research or investigation (\"find where X is defined\", \"investigate Y\")\n\
- Multi-step changes or any background processing\n\n\
Examples:\n\
- User: \"Implement JWT authentication for the REST API\"\n\
  Action: Call dispatch_task with prompt=\"Implement JWT authentication for the REST API\"\n\n\
- User: \"Refactor the auth module into its own crate\"\n\
  Action: Call dispatch_task with prompt=\"Refactor the auth module into its own crate\"\n\n\
- User: \"你自己探索一下当前目录\" / \"Explore the current directory\"\n\
  Action: Call dispatch_task with prompt=\"Explore the current directory and report the project structure\"\n\n\
- User: \"What does this file do?\" / \"这个文件是干什么的\"\n\
  Action: Call dispatch_task with prompt=\"Read and explain what this file does\"\n\n\
- User: \"Hi, how are you?\" / \"你好\"\n\
  Action: Respond directly. Do NOT call dispatch_task (general greeting, no codebase access needed).\n\n\
- User: \"What's the difference between Vec and VecDeque in Rust?\"\n\
  Action: Respond directly. Do NOT call dispatch_task (general knowledge question).\n\n\
RULE: If the user's request references their codebase, files, project, or \
environment → use dispatch_task. If it's a general programming question \
answerable from training data → respond directly. \
When unsure, prefer dispatch_task. \
Do not explain this mechanism to the user.\n\n\
## Tone & Communication\n\n\
- Be concise. Start work immediately. No acknowledgments.\n\
- No flattery: never start with \"Great question!\", \"That's a really good idea!\", etc.\n\
- No status updates: \"I'm on it\", \"Let me start...\", etc. Use heartbeats and todo tracking instead.\n\
- When user is wrong: state concern concisely, propose alternative, ask if they want to proceed.\n\
- Match user's communication style (terse vs detailed).";

/// Returns the Orchestrator system prompt template.
#[must_use]
pub const fn orchestrator_prompt() -> &'static str {
    ORCHESTRATOR_SYSTEM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_does_not_contain_legacy_prefix() {
        assert!(
            !ORCHESTRATOR_SYSTEM.contains("OPCA_DISPATCH"),
            "prompt must not reference the legacy OPCA_DISPATCH prefix"
        );
    }

    #[test]
    fn prompt_contains_dispatch_tool_instructions() {
        assert!(
            ORCHESTRATOR_SYSTEM.contains("dispatch_task"),
            "prompt must reference the dispatch_task tool"
        );
    }

    #[test]
    fn prompt_version_is_v4() {
        assert_eq!(PROMPT_VERSION, "orchestrator-v4");
    }

    #[test]
    fn prompt_states_no_filesystem_access() {
        assert!(
            ORCHESTRATOR_SYSTEM.contains("NO filesystem access"),
            "prompt must state that the orchestrator has no filesystem access"
        );
    }

    #[test]
    fn prompt_covers_exploration_requests() {
        assert!(
            ORCHESTRATOR_SYSTEM.contains("Codebase exploration"),
            "prompt must cover codebase exploration as a dispatch trigger"
        );
    }

    #[test]
    fn prompt_does_not_misguide_file_questions() {
        // The old prompt said "What does this file do? → Respond directly" which
        // is wrong because the orchestrator has no filesystem access.
        assert!(
            !ORCHESTRATOR_SYSTEM.contains(
                "Respond directly. Do NOT call dispatch_task (this is a question, not work)"
            ),
            "prompt must not tell the orchestrator to answer file questions directly"
        );
    }

    #[test]
    fn prompt_contains_tone_policy() {
        assert!(
            ORCHESTRATOR_SYSTEM.contains("Tone & Communication"),
            "prompt must contain the Tone & Communication section"
        );
        assert!(
            ORCHESTRATOR_SYSTEM.contains("No flattery"),
            "prompt must contain the no-flattery rule"
        );
        assert!(
            ORCHESTRATOR_SYSTEM.contains("No status updates"),
            "prompt must contain the no-status-updates rule"
        );
    }
}

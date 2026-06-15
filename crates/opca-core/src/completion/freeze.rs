//! Freeze stage (Task 12.2).
//!
//! When a Task reaches Delivered state, the Freeze stage:
//! 1. Makes the workspace read-only (`workspace.freeze()`).
//! 2. Generates a final summary (~200 tokens).
//! 3. Pushes a "Delivered" heartbeat.
//! 4. Notifies Orchestrator and CLI.
//!
//! See `design.md` §D9 (① Freeze) and `specs/completion-pipeline/spec.md`.

use anyhow::Result;
use tokio_stream::StreamExt;

use crate::lifecycle::TaskStatus;
use crate::provider::{Message, Provider, ProviderEvent};
use crate::workspace::Workspace;

use super::pipeline::FreezeResult;

/// Summarization prompt prefix sent to the provider when generating the
/// final summary. Spec recommends ~200 tokens.
const SUMMARIZE_PROMPT: &str = "Summarize concisely (max ~200 tokens) what you did in this task. \
     Focus on: what changed, why, and any caveats the reviewer should know.";

/// Freeze the Task's workspace and produce a final summary.
///
/// The summary is produced by asking the provider with a "summarize what you
/// did" prompt. If the provider is unavailable or the active context is
/// empty, we fall back to the last assistant message (or a fixed string).
pub async fn freeze(
    workspace: &mut dyn Workspace,
    provider: &dyn Provider,
    active: &[Message],
    heartbeat_tx: &tokio::sync::mpsc::UnboundedSender<crate::lifecycle::Heartbeat>,
    task_id: &str,
) -> Result<FreezeResult> {
    // 1. Freeze workspace.
    workspace.freeze()?;

    // 2. Generate final summary.
    let summary = generate_summary(provider, active).await;

    // 3. Push "Delivered" heartbeat.
    let hb = crate::lifecycle::Heartbeat {
        task_id: task_id.to_string(),
        status: TaskStatus::Delivered,
        progress: 1.0,
        summary: summary.clone(),
        timestamp: 0,
        todo: None,
        subtasks: Vec::new(),
    };
    let _ = heartbeat_tx.send(hb);

    Ok(FreezeResult { summary })
}

async fn generate_summary(provider: &dyn Provider, active: &[Message]) -> String {
    if !active.is_empty() {
        let mut messages: Vec<Message> = active.to_vec();
        messages.push(Message::user(SUMMARIZE_PROMPT));
        if let Ok(stream) = provider.stream(&messages, &[], None).await {
            if let Ok(text) = collect_text(stream).await {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }

    if let Some(last) = active
        .iter()
        .rev()
        .find(|m| m.role == crate::provider::MessageRole::Assistant && !m.content.is_empty())
    {
        return last.content.clone();
    }

    "Task completed (no summary available)".to_string()
}

async fn collect_text(mut stream: crate::provider::ProviderStream) -> Result<String> {
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            Ok(ProviderEvent::TextDelta(delta)) => text.push_str(&delta),
            Ok(ProviderEvent::Done { .. }) => break,
            Ok(ProviderEvent::Error(msg)) => anyhow::bail!(msg),
            Ok(_) => {}
            Err(e) => anyhow::bail!(e.to_string()),
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::CopyWorkspace;

    fn make_workspace() -> (tempfile::TempDir, CopyWorkspace) {
        let project = tempfile::tempdir().expect("tempdir");
        std::fs::write(project.path().join("a.txt"), b"a").expect("write");
        let parent = tempfile::tempdir().expect("tempdir");
        let ws =
            CopyWorkspace::create(project.path(), parent.path(), "freeze-test").expect("create");
        (project, ws)
    }

    struct StubProvider {
        text: String,
    }

    impl Provider for StubProvider {
        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[crate::provider::ToolDef],
            _system_prompt: Option<&str>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = anyhow::Result<crate::provider::ProviderStream>>
                    + Send,
            >,
        > {
            let text = self.text.clone();
            Box::pin(async move {
                use futures::stream;
                let events = vec![
                    Ok(ProviderEvent::TextDelta(text)),
                    Ok(ProviderEvent::Done {
                        stop_reason: crate::provider::StopReason::EndTurn,
                    }),
                ];
                Ok(Box::pin(stream::iter(events)) as crate::provider::ProviderStream)
            })
        }
    }

    #[tokio::test]
    async fn freeze_marks_workspace_readonly() {
        let (_project, mut ws) = make_workspace();
        assert!(!ws.is_frozen());
        let provider = StubProvider {
            text: String::new(),
        };
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = freeze(
            &mut ws as &mut dyn Workspace,
            &provider,
            &[],
            &tx,
            "freeze-test",
        )
        .await
        .expect("freeze");
        assert!(ws.is_frozen());
        assert!(!result.summary.is_empty());
    }

    #[tokio::test]
    async fn freeze_summary_uses_provider_response() {
        let (_project, mut ws) = make_workspace();
        let provider = StubProvider {
            text: "I refactored the auth module to use OAuth2.".to_string(),
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let active = vec![Message::assistant("did some work")];

        let result = freeze(&mut ws as &mut dyn Workspace, &provider, &active, &tx, "t1")
            .await
            .expect("freeze");

        assert_eq!(
            result.summary,
            "I refactored the auth module to use OAuth2."
        );

        let hb = rx.try_recv().expect("heartbeat");
        assert_eq!(hb.status, TaskStatus::Delivered);
        assert_eq!(hb.summary, result.summary);
    }

    #[tokio::test]
    async fn freeze_falls_back_to_last_assistant_message() {
        let (_project, mut ws) = make_workspace();
        let provider = StubProvider {
            text: String::new(),
        };
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let active = vec![
            Message::user("do thing"),
            Message::assistant("final assistant text"),
        ];

        let result = freeze(&mut ws as &mut dyn Workspace, &provider, &active, &tx, "t2")
            .await
            .expect("freeze");

        assert_eq!(result.summary, "final assistant text");
    }

    #[tokio::test]
    async fn freeze_empty_context_default_summary() {
        let (_project, mut ws) = make_workspace();
        let provider = StubProvider {
            text: String::new(),
        };
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let result = freeze(&mut ws as &mut dyn Workspace, &provider, &[], &tx, "t3")
            .await
            .expect("freeze");

        assert!(!result.summary.is_empty());
    }
}

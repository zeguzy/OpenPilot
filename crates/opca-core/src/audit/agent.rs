use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio_stream::StreamExt;

use serde::{Deserialize, Serialize};

use crate::memory::extract_keywords;
use crate::provider::{Message, Provider, ProviderEvent};
use crate::workspace::ChangeSet;

use super::focus::is_diff_suspicious;
use super::report::{AuditReport, AuditVerdict, Finding};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    Cheap,
    Strong,
}

pub struct AuditAgent {
    provider: Arc<dyn Provider>,
    task_id: String,
    workspace_path: PathBuf,
    diff: ChangeSet,
    task_memory: Arc<Mutex<Vec<Message>>>,
    focus: Vec<String>,
    model_tier: ModelTier,
}

impl AuditAgent {
    #[must_use]
    pub fn new(
        provider: Arc<dyn Provider>,
        task_id: impl Into<String>,
        workspace_path: PathBuf,
        diff: ChangeSet,
        task_memory: Arc<Mutex<Vec<Message>>>,
        focus: Vec<String>,
        model_tier: ModelTier,
    ) -> Self {
        Self {
            provider,
            task_id: task_id.into(),
            workspace_path,
            diff,
            task_memory,
            focus,
            model_tier,
        }
    }

    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    #[must_use]
    pub const fn workspace_path(&self) -> &PathBuf {
        &self.workspace_path
    }

    #[must_use]
    pub const fn model_tier(&self) -> ModelTier {
        self.model_tier
    }

    #[must_use]
    pub fn focus(&self) -> &[String] {
        &self.focus
    }

    #[must_use]
    pub const fn diff(&self) -> &ChangeSet {
        &self.diff
    }

    pub async fn audit(&self) -> Result<AuditReport> {
        let system_prompt = self.build_system_prompt();
        let user_msg = self.build_audit_request();

        let messages = vec![Message::user(user_msg)];
        let tools = &[];

        let stream = self
            .provider
            .stream(&messages, tools, Some(&system_prompt))
            .await?;

        let text = self.collect_text(stream).await?;

        let mut report: AuditReport = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("failed to parse audit response: {e}"))?;
        report.task_id.clone_from(&self.task_id);
        Ok(report)
    }

    #[allow(dead_code)]
    pub async fn audit_with_deep_dive(&self) -> Result<AuditReport> {
        let mut report = self.audit().await?;

        if is_diff_suspicious(&self.diff) && report.verdict != AuditVerdict::Confirmed {
            let deleted_query = self
                .diff
                .deleted
                .iter()
                .filter_map(|p| p.to_str())
                .collect::<Vec<_>>()
                .join(" ");
            let context = self.deep_dive_task_context(&deleted_query).await?;
            if !context.is_empty() {
                let reasoning = context
                    .iter()
                    .map(|m| m.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if reasoning_is_flawed(&reasoning) {
                    report.verdict = AuditVerdict::NeedsFix;
                    report.confidence = (report.confidence + 0.1).min(1.0);
                    report.findings.push(Finding {
                        severity: crate::focus::Severity::Blocking,
                        location: "deep-dive".to_string(),
                        issue: format!(
                            "Task reasoning for suspicious deletion appears flawed: {reasoning}"
                        ),
                    });
                }
            }
        }

        Ok(report)
    }

    #[allow(clippy::unused_async)]
    pub async fn deep_dive_task_context(&self, query: &str) -> Result<Vec<Message>> {
        let messages = {
            let memory = self
                .task_memory
                .lock()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let keywords = extract_keywords(query);
            memory
                .iter()
                .filter(|msg| {
                    if keywords.is_empty() {
                        true
                    } else {
                        let content = msg.content.to_lowercase();
                        keywords.iter().any(|kw| content.contains(kw))
                    }
                })
                .cloned()
                .collect::<Vec<Message>>()
        };
        Ok(messages)
    }

    fn build_system_prompt(&self) -> String {
        let dims = self.focus.join(", ");
        format!(
            "You are an Audit Agent reviewing a completed task's diff. \
             You must check these dimensions: [{dims}]. \
             Respond ONLY with a JSON object with fields: \
             verdict (\"confirmed\"|\"false_positive\"|\"needs_fix\"|\"needs_human_review\"), confidence (0.0-1.0), \
             findings (array of {{severity, location, issue}}), and summary (string)."
        )
    }

    fn build_audit_request(&self) -> String {
        let deleted: Vec<String> = self
            .diff
            .deleted
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let modified: Vec<String> = self
            .diff
            .modified
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let added: Vec<String> = self
            .diff
            .added
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        format!(
            "Review the following changes for task {}:\n\
             Added files ({}): {}\n\
             Modified files ({}): {}\n\
             Deleted files ({}): {}\n\n\
             Provide your audit verdict as JSON.",
            self.task_id,
            self.diff.added.len(),
            added.join(", "),
            self.diff.modified.len(),
            modified.join(", "),
            self.diff.deleted.len(),
            deleted.join(", "),
        )
    }

    async fn collect_text(&self, mut stream: crate::provider::ProviderStream) -> Result<String> {
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
}

impl std::fmt::Debug for AuditAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mem_len = self.task_memory.lock().map(|m| m.len()).unwrap_or(0);
        f.debug_struct("AuditAgent")
            .field("task_id", &self.task_id)
            .field("workspace_path", &self.workspace_path)
            .field("diff", &self.diff)
            .field("focus", &self.focus)
            .field("model_tier", &self.model_tier)
            .field("task_memory_len", &mem_len)
            .finish_non_exhaustive()
    }
}

fn reasoning_is_flawed(reasoning: &str) -> bool {
    let lower = reasoning.to_lowercase();
    lower.contains("i think")
        || lower.contains("guess")
        || lower.contains("not sure")
        || lower.contains("might work")
}

use super::registry::ToolRegistry;
use super::tool::ToolContext;
use crate::provider::{ToolCall, ToolEffects, ToolResult};

pub async fn dispatch_batch(
    tools: &ToolRegistry,
    calls: &[ToolCall],
    ctx: &ToolContext,
) -> Vec<(String, anyhow::Result<ToolResult>)> {
    let mut results: Vec<Option<(String, anyhow::Result<ToolResult>)>> =
        (0..calls.len()).map(|_| None).collect();

    let mut parallel_indices: Vec<usize> = Vec::new();
    let mut serial_indices: Vec<usize> = Vec::new();

    for (i, c) in calls.iter().enumerate() {
        match tools.get(&c.name) {
            Some(t) => match t.effects() {
                ToolEffects::Read | ToolEffects::Append => parallel_indices.push(i),
                ToolEffects::Write | ToolEffects::Process => serial_indices.push(i),
            },
            None => {
                results[i] = Some((
                    c.id.clone(),
                    Err(anyhow::anyhow!("unknown tool: {}", c.name)),
                ));
            }
        }
    }

    let futs: Vec<_> = parallel_indices
        .iter()
        .map(|&i| {
            let c = &calls[i];
            async move {
                let result = tools.execute(&c.name, &c.arguments, ctx).await;
                (i, c.id.clone(), result)
            }
        })
        .collect();
    let par_results = futures::future::join_all(futs).await;
    for (i, id, result) in par_results {
        results[i] = Some((id, result));
    }

    for &i in &serial_indices {
        let c = &calls[i];
        let result = tools.execute(&c.name, &c.arguments, ctx).await;
        results[i] = Some((c.id.clone(), result));
    }

    results
        .into_iter()
        .map(|x| x.expect("all slots must be filled"))
        .collect()
}

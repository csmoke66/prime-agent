//! Agent-selected projection of durable tool history into model context.
//! Resolve relative tail requests before persistence; a compacted or resumed
//! history must never reinterpret a suffix against different neighbors.

use bh_agent::types as live;
use bh_types::{ai, session};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(super) enum ReleaseToolOutputs<'a> {
    Last,
    All,
    Current,
    Explicit(&'a [Value]),
    Exchanges(&'a [Value]),
}

pub(super) fn release_directive<'a>(
    name: &str,
    is_error: bool,
    details: Option<&'a Value>,
) -> Option<ReleaseToolOutputs<'a>> {
    if is_error || name != ai::PYTHON_TOOL_NAME {
        return None;
    }
    match details?.get("releaseToolOutputs")? {
        Value::String(s) => match s.as_str() {
            "last" => Some(ReleaseToolOutputs::Last),
            "all" => Some(ReleaseToolOutputs::All),
            "current" => Some(ReleaseToolOutputs::Current),
            _ => None,
        },
        Value::Array(ids)
            if !ids.is_empty() && ids.iter().all(|id| id.as_str().is_some_and(|s| !s.is_empty())) =>
        {
            Some(ReleaseToolOutputs::Explicit(ids))
        }
        Value::Object(obj) => Some(ReleaseToolOutputs::Exchanges(
            obj.get("exchanges")?.as_array()?,
        )),
        _ => None,
    }
}

pub(super) fn loop_release_directive(
    result: &live::ToolResultMessage,
) -> Option<ReleaseToolOutputs<'_>> {
    release_directive(&result.tool_name, result.is_error, result.details.as_ref())
}

// A provider can reuse an ID in a later turn. Qualify both ends of a call
// with their persisted timestamps so the unchanged prefix stays unchanged.
fn exchange_key(record: &Value) -> Option<(&str, u64, u64)> {
    let [id, assistant_ts, result_ts] = record.as_array()?.as_slice() else {
        return None;
    };
    Some((id.as_str()?, assistant_ts.as_u64()?, result_ts.as_u64()?))
}

pub(super) fn released_exchange_keys(records: &[Value]) -> impl Iterator<Item = (&str, u64, u64)> {
    records.iter().filter_map(exchange_key)
}

fn call_ids(assistant: &live::AssistantMessage) -> impl Iterator<Item = &str> {
    assistant.content.iter().filter_map(|block| match block {
        live::AssistantContent::ToolCall(call) => Some(call.id.as_str()),
        _ => None,
    })
}

fn tool_result(row: &live::AgentMessage) -> Option<&live::ToolResultMessage> {
    match row {
        live::AgentMessage::Standard(live::Message::ToolResult(result)) => Some(result),
        _ => None,
    }
}

/// Skip this assistant's entire batch (including siblings that landed before
/// the release). Stop at the first non-tool turn or incomplete exchange.
fn tail_targets(prefix: &[live::AgentMessage], current_id: &str, mut batches: u64) -> Vec<Value> {
    let mut targets = Vec::new();
    let Some((live::AgentMessage::Standard(live::Message::Assistant(current)), history)) =
        prefix.split_last()
    else {
        return targets;
    };
    if !call_ids(current).any(|id| id == current_id) {
        return targets;
    }
    let prior = loop_release_mask(history);
    let mut end = history.len();
    let mut waiting = HashSet::new();
    while batches > 0 {
        let Some(owner) = history[..end].iter().rposition(|row| tool_result(row).is_none()) else {
            break;
        };
        let live::AgentMessage::Standard(live::Message::Assistant(assistant)) = &history[owner] else {
            break;
        };
        let results = &history[owner + 1..end];
        if results.is_empty() {
            break;
        }
        end = owner;
        if effect(&prior, owner) == ReleaseEffect::Omit {
            continue; // This exchange was not part of the previous provider input.
        }
        let Ok(assistant_ts) = u64::try_from(assistant.timestamp) else {
            break;
        };
        // All calls must be paired exactly once, in any result order. Never
        // take half of a parallel/signed assistant response or a partial batch.
        if !call_ids(assistant).all(|id| !id.is_empty() && waiting.insert(id))
            || waiting.len() != results.len()
            || !results
                .iter()
                .filter_map(tool_result)
                .all(|r| r.timestamp >= 0 && waiting.remove(r.tool_call_id.as_str()))
        {
            break;
        }
        targets.extend(results.iter().filter_map(tool_result).map(|r| {
            json!([r.tool_call_id, assistant_ts, r.timestamp])
        }));
        batches -= 1;
    }
    targets
}

pub(super) fn resolve_loop_tail_release(
    result: &mut live::ToolResultMessage,
    prefix: &[live::AgentMessage],
) {
    if result.is_error || result.tool_name != ai::PYTHON_TOOL_NAME {
        return;
    }
    let Some(directive) = result.details.as_mut().and_then(|d| d.get_mut("releaseToolOutputs")) else {
        return;
    };
    if let Some(n) = directive.get("tail").and_then(Value::as_u64) {
        *directive = json!({"exchanges": tail_targets(prefix, &result.tool_call_id, n)});
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ReleaseEffect {
    #[default]
    Keep,
    Body,
    Omit,
}

pub(super) fn effect(mask: &Option<Vec<ReleaseEffect>>, index: usize) -> ReleaseEffect {
    mask.as_ref().map_or(ReleaseEffect::Keep, |m| m[index])
}

enum Row<'a> {
    Result(&'a str, Option<u64>, Option<ReleaseToolOutputs<'a>>),
    SessionAssistant(&'a ai::AssistantMessage),
    LiveAssistant(&'a live::AssistantMessage),
    Other,
}

fn session_row(message: &session::AgentMessage) -> Row<'_> {
    match message {
        session::AgentMessage::ToolResult(r) => Row::Result(
            &r.tool_call_id,
            Some(r.timestamp),
            release_directive(&r.tool_name, r.is_error, r.details.as_ref()),
        ),
        session::AgentMessage::Assistant(a) => Row::SessionAssistant(a),
        _ => Row::Other,
    }
}

fn live_row(message: &live::AgentMessage) -> Row<'_> {
    match message {
        live::AgentMessage::Standard(live::Message::ToolResult(r)) => Row::Result(
            &r.tool_call_id,
            u64::try_from(r.timestamp).ok(),
            loop_release_directive(r),
        ),
        live::AgentMessage::Standard(live::Message::Assistant(a)) => Row::LiveAssistant(a),
        _ => Row::Other,
    }
}

fn owns_exchange<'a>(
    mut ids: impl Iterator<Item = &'a str>,
    ts: u64,
    calls: &HashSet<(&str, u64)>,
) -> bool {
    ids.next().is_some_and(|id| calls.contains(&(id, ts))) && ids.all(|id| calls.contains(&(id, ts)))
}

fn release_mask<'a>(
    len: usize,
    rows: impl DoubleEndedIterator<Item = (usize, Row<'a>)>,
) -> Option<Vec<ReleaseEffect>> {
    use ReleaseEffect::{Body, Keep, Omit};
    let mut mask = None;
    let (mut last, mut all) = (false, false);
    let mut ids = HashSet::new();
    let mut calls = HashSet::new();
    let mut results = HashSet::new();
    // Walk backward: last/all/IDs/exchanges never acquire future results.
    // No release: no allocations and no scan through tool arguments or bodies.
    for (index, row) in rows.rev() {
        let action = match row {
            Row::Result(id, ts, directive) => {
                let action = if !results.is_empty() && ts.is_some_and(|ts| results.contains(&(id, ts))) {
                    Omit
                } else if all || last || matches!(directive, Some(ReleaseToolOutputs::Current))
                    || ids.contains(id)
                {
                    Body
                } else {
                    Keep
                };
                last = matches!(directive, Some(ReleaseToolOutputs::Last));
                match directive {
                    Some(ReleaseToolOutputs::All) => all = true,
                    Some(ReleaseToolOutputs::Explicit(targets)) if !all => {
                        ids.extend(targets.iter().filter_map(Value::as_str));
                    }
                    Some(ReleaseToolOutputs::Exchanges(records)) => {
                        for (id, assistant_ts, result_ts) in released_exchange_keys(records) {
                            calls.insert((id, assistant_ts));
                            results.insert((id, result_ts));
                        }
                    }
                    _ => {}
                }
                action
            }
            Row::SessionAssistant(a) if !calls.is_empty() => {
                let ids = a.content.iter().filter_map(|block| match block {
                    ai::AssistantContentBlock::ToolCall(call) => Some(call.id.as_str()),
                    _ => None,
                });
                if owns_exchange(ids, a.timestamp, &calls) { Omit } else { Keep }
            }
            Row::LiveAssistant(a) if !calls.is_empty() => {
                if u64::try_from(a.timestamp).is_ok_and(|ts| owns_exchange(call_ids(a), ts, &calls)) {
                    Omit
                } else {
                    Keep
                }
            }
            _ => Keep,
        };
        if action != Keep {
            mask.get_or_insert_with(|| vec![Keep; len])[index] = action;
        }
    }
    mask
}

pub(super) fn session_release_mask(messages: &[session::AgentMessage]) -> Option<Vec<ReleaseEffect>> {
    release_mask(messages.len(), messages.iter().map(session_row).enumerate())
}

pub(super) fn loop_release_mask(messages: &[live::AgentMessage]) -> Option<Vec<ReleaseEffect>> {
    release_mask(messages.len(), messages.iter().map(live_row).enumerate())
}

pub(super) fn entry_release_mask(entries: &[session::FileEntry]) -> Option<Vec<ReleaseEffect>> {
    let rows = entries.iter().map(|entry| match entry {
        session::FileEntry::Message { message, .. } => session_row(message),
        _ => Row::Other,
    });
    release_mask(entries.len(), rows.enumerate())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(ts: i64, ids: &[&str]) -> live::AgentMessage {
        let mut blocks = vec![json!({"type":"thinking","thinking":"reasoning","thinkingSignature":"signed"})];
        blocks.extend(ids.iter().map(|id| json!({
            "type":"toolCall", "id":id, "name":"python_repl", "arguments":{"code":"work()"}
        })));
        serde_json::from_value::<live::AssistantMessage>(json!({
            "timestamp":ts, "content":blocks, "usage":{}, "stopReason":"toolUse"
        })).unwrap().into()
    }

    fn result(ts: i64, id: &str) -> live::ToolResultMessage {
        live::ToolResultMessage {
            tool_call_id: id.into(),
            tool_name: ai::PYTHON_TOOL_NAME.into(),
            content: vec![live::ToolResultContent::Image(live::ImageContent {
                data: "full-image".into(),
                mime_type: "image/png".into(),
            })],
            details: None,
            is_error: false,
            timestamp: ts,
        }
    }

    #[test]
    fn tail_trims_complete_signed_batches_and_freezes_exact_targets() {
        let user: live::AgentMessage = live::UserMessage {
            content: live::UserContent::Text("keep the boundary".into()), timestamp: 3,
        }.into();
        let mut history = vec![
            assistant(1, &["reused"]), result(2, "reused").into(), user,
            assistant(4, &["reused", "parallel"]), result(5, "parallel").into(),
            result(6, "reused").into(), assistant(7, &["next"]), result(8, "next").into(),
            assistant(9, &["sibling", "release"]),
        ];
        let mut directive = result(11, "release");
        directive.details = Some(json!({"releaseToolOutputs": {"tail": 100}}));
        resolve_loop_tail_release(&mut directive, &history);
        let keep = history[..3].to_vec();
        history.push(result(10, "sibling").into());
        history.push(directive.into());
        let mut expected = keep.clone();
        expected.extend_from_slice(&history[8..]);
        super::super::messages::project_loop_history_tool_output_releases(&mut history);
        assert_eq!(history, expected, "neither a repeated ID across a user turn nor a current sibling belongs to the tail");
        // Reapplying the persisted directive after its original targets have
        // disappeared cannot truncate different rows.
        super::super::messages::project_loop_history_tool_output_releases(&mut history);
        assert_eq!(history, expected);
    }

    #[test]
    fn incomplete_parallel_tail_is_not_cut_or_crossed() {
        let history = vec![
            assistant(1, &["done"]), result(2, "done").into(),
            assistant(3, &["a", "missing"]), result(4, "a").into(),
            assistant(5, &["release"]),
        ];
        let mut directive = result(6, "release");
        directive.details = Some(json!({"releaseToolOutputs":{"tail":5}}));
        resolve_loop_tail_release(&mut directive, &history);
        let mut after = history.clone();
        after.push(directive.into());
        let mut visible = after.clone();
        super::super::messages::project_loop_history_tool_output_releases(&mut visible);
        assert_eq!(visible, after);
    }
}

//! Attach-stream rendering and salvage of unparsable client lines.
use super::{
    response_failure, response_line, stream_attach, DaemonResponse, SnapshotPurpose, Value,
};

pub(super) fn streamed_attach_lines(
    mut response: DaemonResponse,
    active_session_id: &str,
    purpose: SnapshotPurpose,
) -> (Vec<Value>, bool) {
    let Some(data) = response.data.take() else {
        return (vec![response_line(&response)], false);
    };
    match stream_attach(data, active_session_id, purpose) {
        Ok((streamed, events)) => {
            response.data = Some(streamed);
            let mut lines = vec![response_line(&response)];
            lines.extend(events.lines());
            (lines, false)
        }
        // The snapshot could not even be identified: the attach itself
        // fails, before any snapshot record exists on the wire.
        Err(error) => (
            vec![response_line(&response_failure(
                response.id.as_deref(),
                &response.command,
                &error.to_string(),
                None,
            ))],
            false,
        ),
    }
}

/// The request id of an unparsable line, so the failure stays matchable by
/// the client (TS `salvageDaemonCommandId`).
pub(super) fn salvage_id(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| value.get("id").and_then(Value::as_str).map(str::to_string))
}

/// The command type of an unparsable line, salvaged for the failure echo
/// (bare commands carry the type at the top level; envelopes nest it).
pub(super) fn salvage_command_type(line: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    value
        .get("command")
        .unwrap_or(&value)
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string)
}

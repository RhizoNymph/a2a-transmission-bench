//! Why a response stopped, as `Response::stop` writes it. The names are
//! crosstalk-spec's `StopReason` in snake case.

/// Why the model stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Refusal,
    /// The server aborted generation.
    Aborted,
    Other,
}

impl StopReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EndTurn => "end_turn",
            Self::ToolUse => "tool_use",
            Self::MaxTokens => "max_tokens",
            Self::StopSequence => "stop_sequence",
            Self::Refusal => "refusal",
            Self::Aborted => "aborted",
            Self::Other => "other",
        }
    }

    /// The stop of a recorded assistant message: a tool use when it calls a
    /// tool, else the end of its turn.
    pub const fn for_calls(calls_tool: bool) -> Self {
        if calls_tool {
            Self::ToolUse
        } else {
            Self::EndTurn
        }
    }
}

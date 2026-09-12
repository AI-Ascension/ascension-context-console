// SPDX-License-Identifier: MIT

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCapture<'a> {
    pub snapshot_id: &'a str,
    pub attempt_id: &'a str,
    pub boundary: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureRecord {
    pub snapshot_id: String,
    pub attempt_id: String,
    pub boundary: String,
    pub state: TransportState,
    pub observed_bytes: usize,
    pub digest: Option<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportState {
    Prepared,
    WriteCompleted,
    WriteFailed,
    ReceiptReported,
    Unknown,
}

impl TransportState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::WriteCompleted => "input_write_completed",
            Self::WriteFailed => "input_write_failed",
            Self::ReceiptReported => "provider_receipt_reported",
            Self::Unknown => "unknown",
        }
    }
}

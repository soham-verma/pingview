//! Splits a raw serial byte stream into MAVLink v1/v2 frames.
//!
//! Only messages whose CRC extra byte we know can be checksum-verified.
//! Everything else the Ping sends (its own status messages) is accepted as
//! an unverified frame only when its declared length lands exactly on the
//! next start byte, so stray 0xFE/0xFD bytes inside a payload don't throw
//! the framer off.

pub const STX_V1: u8 = 0xFE;
pub const STX_V2: u8 = 0xFD;

const V1_HEADER: usize = 6;
const V2_HEADER: usize = 10;
const CRC_LEN: usize = 2;
const V2_SIGNATURE_LEN: usize = 13;
const V2_FLAG_SIGNED: u8 = 0x01;

/// CRC extra byte for each message ID PingView can verify.
pub fn crc_extra(msgid: u32) -> Option<u8> {
    match msgid {
        crate::adsb::MSG_ID_ADSB_VEHICLE => Some(crate::adsb::CRC_EXTRA),
        _ => None,
    }
}

/// MAVLink's X.25 (CRC-16/MCRF4XX) over `data`, then the CRC extra byte.
pub fn x25_crc(data: &[u8], extra: u8) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data.iter().chain(std::iter::once(&extra)) {
        let mut tmp = b ^ (crc & 0xFF) as u8;
        tmp ^= tmp << 4;
        crc = (crc >> 8) ^ ((tmp as u16) << 8) ^ ((tmp as u16) << 3) ^ ((tmp as u16) >> 4);
    }
    crc
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub version: u8,
    pub seq: u8,
    pub sysid: u8,
    pub compid: u8,
    pub msgid: u32,
    pub payload: Vec<u8>,
    /// True when the checksum was checked and matched; false when the
    /// message ID is unknown, so only its framing was checked.
    pub verified: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct FramerStats {
    pub verified: u64,
    pub other_frames: u64,
    pub crc_errors: u64,
}

#[derive(Default)]
pub struct Framer {
    buf: Vec<u8>,
    pub stats: FramerStats,
}

enum Step {
    Frame(Frame, usize),
    NeedMore,
    Skip,
}

impl Framer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds bytes in and returns every complete frame they finish.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Frame> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            match self.buf.iter().position(|&b| b == STX_V1 || b == STX_V2) {
                Some(0) => {}
                Some(i) => {
                    self.buf.drain(..i);
                }
                None => {
                    self.buf.clear();
                    break;
                }
            }
            match self.try_frame() {
                Step::Frame(frame, len) => {
                    self.buf.drain(..len);
                    if frame.verified {
                        self.stats.verified += 1;
                    } else {
                        self.stats.other_frames += 1;
                    }
                    out.push(frame);
                }
                Step::Skip => {
                    self.buf.drain(..1);
                }
                Step::NeedMore => break,
            }
        }
        out
    }

    fn try_frame(&mut self) -> Step {
        let b = &self.buf;
        let (header, msgid, seq, sysid, compid, extra_len) = match b[0] {
            STX_V1 => {
                if b.len() < V1_HEADER {
                    return Step::NeedMore;
                }
                (V1_HEADER, b[5] as u32, b[2], b[3], b[4], 0)
            }
            _ => {
                if b.len() < V2_HEADER {
                    return Step::NeedMore;
                }
                let signed = if b[2] & V2_FLAG_SIGNED != 0 { V2_SIGNATURE_LEN } else { 0 };
                let msgid = b[7] as u32 | (b[8] as u32) << 8 | (b[9] as u32) << 16;
                (V2_HEADER, msgid, b[4], b[5], b[6], signed)
            }
        };
        let payload_len = b[1] as usize;
        let total = header + payload_len + CRC_LEN + extra_len;
        if b.len() < total {
            return Step::NeedMore;
        }
        let crc_at = header + payload_len;
        let wire_crc = u16::from_le_bytes([b[crc_at], b[crc_at + 1]]);
        let verified = match crc_extra(msgid) {
            Some(extra) => {
                if x25_crc(&b[1..crc_at], extra) != wire_crc {
                    self.stats.crc_errors += 1;
                    return Step::Skip;
                }
                true
            }
            None => {
                // Unknown message: trust the length only if the next frame
                // starts right where this one says it ends.
                match b.get(total) {
                    None => return Step::NeedMore,
                    Some(&next) if next == STX_V1 || next == STX_V2 => false,
                    Some(_) => return Step::Skip,
                }
            }
        };
        let frame = Frame {
            version: if b[0] == STX_V1 { 1 } else { 2 },
            seq,
            sysid,
            compid,
            msgid,
            payload: b[header..crc_at].to_vec(),
            verified,
        };
        Step::Frame(frame, total)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a valid v1 frame for a message we can verify.
    pub fn v1_frame(seq: u8, msgid: u8, payload: &[u8], extra: u8) -> Vec<u8> {
        let mut f = vec![STX_V1, payload.len() as u8, seq, 1, 0x9C, msgid];
        f.extend_from_slice(payload);
        let crc = x25_crc(&f[1..], extra);
        f.extend_from_slice(&crc.to_le_bytes());
        f
    }

    fn v2_frame(msgid: u32, payload: &[u8], extra: u8) -> Vec<u8> {
        let mut f = vec![STX_V2, payload.len() as u8, 0, 0, 7, 1, 0x9C];
        f.extend_from_slice(&msgid.to_le_bytes()[..3]);
        f.extend_from_slice(payload);
        let crc = x25_crc(&f[1..], extra);
        f.extend_from_slice(&crc.to_le_bytes());
        f
    }

    #[test]
    fn x25_matches_mavlink_reference_value() {
        // MAVLink HEARTBEAT (msg 0, CRC extra 50) from the MAVLink docs:
        // FE 09 4E 01 01 00 | 00 00 00 00 02 03 51 04 03 | CRC 1C 7F
        let frame = [
            0xFE, 0x09, 0x4E, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x03, 0x51, 0x04,
            0x03,
        ];
        assert_eq!(x25_crc(&frame[1..], 50), u16::from_le_bytes([0x1C, 0x7F]));
    }

    #[test]
    fn verifies_known_message() {
        let payload = [7u8; 38];
        let mut framer = Framer::new();
        let frames = framer.push(&v1_frame(5, 246, &payload, 184));
        assert_eq!(frames.len(), 1);
        assert!(frames[0].verified);
        assert_eq!(frames[0].msgid, 246);
        assert_eq!(frames[0].seq, 5);
        assert_eq!(frames[0].payload, payload);
        assert_eq!(framer.stats.verified, 1);
    }

    #[test]
    fn rejects_bad_checksum_and_resyncs() {
        let mut bad = v1_frame(1, 246, &[1u8; 38], 184);
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        let good = v1_frame(2, 246, &[2u8; 38], 184);
        let mut framer = Framer::new();
        let frames = framer.push(&[bad, good].concat());
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].seq, 2);
        assert_eq!(framer.stats.crc_errors, 1);
    }

    #[test]
    fn skips_leading_garbage_without_losing_frames() {
        // The stray 0xFE claims a 153-byte frame, so the framer waits until
        // that many bytes arrive before rejecting it and rescanning.
        let mut stream = vec![0x00, 0x13, 0xFE, 0x99];
        for seq in 0..5 {
            stream.extend(v1_frame(seq, 246, &[seq; 38], 184));
        }
        let frames = Framer::new().push(&stream);
        let seqs: Vec<u8> = frames.iter().map(|f| f.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn frame_split_at_every_byte_gives_same_result() {
        let stream = [
            v1_frame(1, 246, &[1u8; 38], 184),
            v1_frame(2, 246, &[2u8; 38], 184),
        ]
        .concat();
        for split in 0..stream.len() {
            let mut framer = Framer::new();
            let mut frames = framer.push(&stream[..split]);
            frames.extend(framer.push(&stream[split..]));
            assert_eq!(frames.len(), 2, "split at {split}");
        }
    }

    #[test]
    fn unknown_message_needs_next_start_byte() {
        let status = v1_frame(1, 203, &[0x00], 0);
        let mut framer = Framer::new();
        assert!(framer.push(&status).is_empty(), "waits for the next frame");
        let frames = framer.push(&v1_frame(2, 246, &[0u8; 38], 184));
        assert_eq!(frames.len(), 2);
        assert!(!frames[0].verified);
        assert_eq!((frames[0].sysid, frames[0].compid), (1, 0x9C));
        assert_eq!(framer.stats.other_frames, 1);
    }

    #[test]
    fn unknown_message_with_wrong_length_is_skipped() {
        let mut fake = vec![STX_V1, 2, 0, 1, 0x9C, 203, 0xAA, 0xBB, 0, 0, 0x55];
        fake.extend(v1_frame(9, 246, &[9u8; 38], 184));
        let frames = Framer::new().push(&fake);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].seq, 9);
    }

    #[test]
    fn decodes_v2_frame_with_truncated_payload() {
        let frames = Framer::new().push(&v2_frame(246, &[1u8; 20], 184));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].version, 2);
        assert_eq!(frames[0].payload.len(), 20);
        assert!(frames[0].verified);
    }
}

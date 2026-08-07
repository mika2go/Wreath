use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedPacket {
    pub track: TrackKind,
    pub timestamp: Duration,
    pub duration: Duration,
    pub keyframe: bool,
    pub payload: std::sync::Arc<[u8]>,
}

impl EncodedPacket {
    pub fn end_timestamp(&self) -> Duration {
        self.timestamp.saturating_add(self.duration)
    }

    fn starts_decodable_video(&self) -> bool {
        self.track == TrackKind::Video && self.keyframe
    }
}

/// Keeps encoded media in memory without ever exceeding its payload budget.
///
/// The front of a non-empty buffer is always a video keyframe. Duration
/// trimming advances by whole groups of pictures, so a saved clip never starts
/// with undecodable delta frames. A single long GOP may temporarily exceed the
/// duration target, but never the byte budget.
#[derive(Debug)]
pub struct EncodedReplayBuffer {
    packets: VecDeque<EncodedPacket>,
    target_duration: Duration,
    max_payload_bytes: usize,
    payload_bytes: usize,
}

impl EncodedReplayBuffer {
    pub fn new(target_duration: Duration, max_payload_bytes: usize) -> Option<Self> {
        if target_duration.is_zero() || max_payload_bytes == 0 {
            return None;
        }

        Some(Self {
            packets: VecDeque::new(),
            target_duration,
            max_payload_bytes,
            payload_bytes: 0,
        })
    }

    /// Adds one encoded packet. Returns false when the packet cannot be kept.
    pub fn push(&mut self, packet: EncodedPacket) -> bool {
        let packet_bytes = packet.payload.len();
        if packet_bytes > self.max_payload_bytes {
            self.clear();
            return false;
        }

        if self.packets.is_empty() && !packet.starts_decodable_video() {
            return false;
        }

        self.payload_bytes += packet_bytes;
        self.packets.push_back(packet);
        self.trim_to_byte_budget();
        self.trim_to_duration();

        self.packets.back().is_some()
    }

    pub fn packets(&self) -> impl ExactSizeIterator<Item = &EncodedPacket> {
        self.packets.iter()
    }

    /// Takes a cheap immutable view for asynchronous muxing. Encoded payloads
    /// are reference-counted, so saving does not copy the replay's video data.
    pub fn snapshot(&self) -> Vec<EncodedPacket> {
        self.packets.iter().cloned().collect()
    }

    pub fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    pub fn duration(&self) -> Duration {
        match (self.packets.front(), self.packets.back()) {
            (Some(first), Some(last)) => last.end_timestamp().saturating_sub(first.timestamp),
            _ => Duration::ZERO,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    /// Starts a new capture epoch without changing the configured limits.
    /// This is used after timestamp discontinuities such as pause/resume.
    pub fn reset(&mut self) {
        self.clear();
    }

    fn trim_to_byte_budget(&mut self) {
        while self.payload_bytes > self.max_payload_bytes {
            if !self.advance_to_next_keyframe() {
                self.clear();
            }
        }
    }

    fn trim_to_duration(&mut self) {
        while self.duration() > self.target_duration {
            if !self.advance_to_next_keyframe() {
                break;
            }
        }
    }

    fn advance_to_next_keyframe(&mut self) -> bool {
        let Some(next_keyframe) = self
            .packets
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, packet)| packet.starts_decodable_video().then_some(index))
        else {
            return false;
        };

        for _ in 0..next_keyframe {
            self.pop_front();
        }
        true
    }

    fn pop_front(&mut self) {
        if let Some(packet) = self.packets.pop_front() {
            self.payload_bytes -= packet.payload.len();
        }
    }

    fn clear(&mut self) {
        self.packets.clear();
        self.payload_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(second: u64, keyframe: bool, bytes: usize) -> EncodedPacket {
        EncodedPacket {
            track: TrackKind::Video,
            timestamp: Duration::from_secs(second),
            duration: Duration::from_secs(1),
            keyframe,
            payload: vec![0; bytes].into(),
        }
    }

    fn audio(second: u64, bytes: usize) -> EncodedPacket {
        EncodedPacket {
            track: TrackKind::Audio,
            timestamp: Duration::from_secs(second),
            duration: Duration::from_secs(1),
            keyframe: false,
            payload: vec![0; bytes].into(),
        }
    }

    #[test]
    fn waits_for_a_video_keyframe_before_retaining_packets() {
        let mut buffer = EncodedReplayBuffer::new(Duration::from_secs(30), 100).unwrap();

        assert!(!buffer.push(audio(0, 10)));
        assert!(!buffer.push(video(0, false, 10)));
        assert!(buffer.push(video(1, true, 10)));
        assert!(buffer.push(audio(1, 10)));

        assert_eq!(buffer.packets().len(), 2);
        assert!(buffer.packets().next().unwrap().starts_decodable_video());
    }

    #[test]
    fn duration_trimming_advances_to_a_keyframe() {
        let mut buffer = EncodedReplayBuffer::new(Duration::from_secs(3), 1_000).unwrap();

        buffer.push(video(0, true, 10));
        buffer.push(video(1, false, 10));
        buffer.push(audio(1, 10));
        buffer.push(video(2, true, 10));
        buffer.push(video(3, false, 10));

        assert_eq!(
            buffer.packets().next().unwrap().timestamp,
            Duration::from_secs(2)
        );
        assert_eq!(buffer.duration(), Duration::from_secs(2));
    }

    #[test]
    fn byte_budget_is_strict_even_without_another_keyframe() {
        let mut buffer = EncodedReplayBuffer::new(Duration::from_secs(30), 25).unwrap();

        assert!(buffer.push(video(0, true, 10)));
        assert!(buffer.push(video(1, false, 10)));
        assert!(!buffer.push(audio(1, 10)));

        assert!(buffer.is_empty());
        assert_eq!(buffer.payload_bytes(), 0);
    }

    #[test]
    fn byte_pressure_discards_complete_old_groups_of_pictures() {
        let mut buffer = EncodedReplayBuffer::new(Duration::from_secs(30), 35).unwrap();

        buffer.push(video(0, true, 10));
        buffer.push(video(1, false, 10));
        buffer.push(video(2, true, 10));
        buffer.push(audio(2, 10));

        assert_eq!(buffer.payload_bytes(), 20);
        assert_eq!(
            buffer.packets().next().unwrap().timestamp,
            Duration::from_secs(2)
        );
    }

    #[test]
    fn rejects_invalid_limits_and_oversized_packets() {
        assert!(EncodedReplayBuffer::new(Duration::ZERO, 10).is_none());
        assert!(EncodedReplayBuffer::new(Duration::from_secs(1), 0).is_none());

        let mut buffer = EncodedReplayBuffer::new(Duration::from_secs(1), 5).unwrap();
        assert!(!buffer.push(video(0, true, 6)));
        assert!(buffer.is_empty());
    }

    #[test]
    fn reset_starts_a_new_decodable_epoch() {
        let mut buffer = EncodedReplayBuffer::new(Duration::from_secs(30), 100).unwrap();
        assert!(buffer.push(video(0, true, 10)));
        assert!(buffer.push(audio(0, 10)));

        buffer.reset();

        assert!(buffer.is_empty());
        assert_eq!(buffer.payload_bytes(), 0);
        assert!(!buffer.push(video(100, false, 10)));
        assert!(buffer.push(video(101, true, 10)));
    }

    #[test]
    fn snapshot_shares_encoded_payload_without_copying_it() {
        let mut buffer = EncodedReplayBuffer::new(Duration::from_secs(30), 100).unwrap();
        assert!(buffer.push(video(0, true, 10)));

        let snapshot = buffer.snapshot();
        let retained = buffer.packets().next().unwrap();

        assert!(std::sync::Arc::ptr_eq(
            &snapshot[0].payload,
            &retained.payload
        ));
    }
}

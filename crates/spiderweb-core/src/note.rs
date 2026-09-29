//! The compact note data model ([`Note`]), replacing the int64 NumPy rows of the Python original.

/// One note, 16 bytes. The Python original used int64 NumPy rows (48 bytes); ticks fit in u32,
/// key / velocity / slot in u8, owner in u32.
///
/// Saturating conversions: ticks below 0 become 0 and ticks above [`u32::MAX`] saturate to it;
/// key stays in `0..=255` (256-key mode), velocity in `0..=127`, slot in `0..=254` (slots above
/// that would already share channels modulo 15) and owner in `0..=u32::MAX`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Note {
    /// Start tick, `>= 0` (saturates at [`u32::MAX`]).
    pub start: u32,
    /// End tick, `>= start` (zero-length notes exist; saturates at [`u32::MAX`]).
    pub end: u32,
    /// Key / pitch, `0..=255` (256-key mode).
    pub key: u8,
    /// Velocity, `1..=127` for rendered notes (0 for a note-off row).
    pub vel: u8,
    /// Channel slot, capped at 254 (more slots already share channels modulo 15).
    pub slot: u8,
    /// Reserved for future flags, 0 for now.
    pub flags: u8,
    /// Index of the owning shape.
    pub owner: u32,
}

/// Saturates an i64 tick into the stored u32 range.
fn tick(v: i64) -> u32 {
    v.clamp(0, u32::MAX as i64) as u32
}

impl Note {
    /// Builds a note from `(start, end, key, velocity)` values, saturating out-of-range ones
    /// (see the type docs). Slot, flags and owner start at 0; `engine::render` fills slot / owner.
    pub fn new(start: i64, end: i64, key: i64, vel: i64) -> Self {
        Self {
            start: tick(start),
            end: tick(end),
            key: key.clamp(0, 255) as u8,
            vel: vel.clamp(0, 127) as u8,
            slot: 0,
            flags: 0,
            owner: 0,
        }
    }

    /// The note as an `(start, end, key, velocity, slot, owner)` i64 row, the pre-compaction form
    /// still used by the MIDI writer, the Domino codec and the differential vectors.
    pub fn row6(self) -> [i64; 6] {
        [
            self.start as i64,
            self.end as i64,
            self.key as i64,
            self.vel as i64,
            self.slot as i64,
            self.owner as i64,
        ]
    }

    /// Reads an `(start, end, key, velocity, slot, owner)` i64 row, saturating out-of-range
    /// values (see the type docs).
    pub fn from_row6(r: [i64; 6]) -> Self {
        Self {
            start: tick(r[0]),
            end: tick(r[1]),
            key: r[2].clamp(0, 255) as u8,
            vel: r[3].clamp(0, 127) as u8,
            slot: r[4].clamp(0, 254) as u8,
            flags: 0,
            owner: r[5].clamp(0, u32::MAX as i64) as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_is_16_bytes() {
        assert_eq!(std::mem::size_of::<Note>(), 16);
        assert_eq!(std::mem::offset_of!(Note, start), 0);
        assert_eq!(std::mem::offset_of!(Note, key), 8);
        assert_eq!(std::mem::offset_of!(Note, owner), 12);
    }

    #[test]
    fn new_defaults_slot_flags_owner() {
        let n = Note::new(960, 1920, 60, 100);
        assert_eq!((n.start, n.end, n.key, n.vel), (960, 1920, 60, 100));
        assert_eq!(n.slot, 0);
        assert_eq!(n.flags, 0);
        assert_eq!(n.owner, 0);
    }

    #[test]
    fn row6_round_trips() {
        let rows = [
            [0, 0, 0, 0, 0, 0],
            [960, 1920, 60, 127, 14, 3],
            [7, 8, 255, 1, 254, 1_000_000],
        ];
        for r in rows {
            assert_eq!(Note::from_row6(r).row6(), r);
        }
    }

    #[test]
    fn conversions_saturate() {
        // Ticks below 0 / above u32::MAX clamp into the stored range
        let n = Note::new(-5, i64::MAX, -1, 300);
        assert_eq!(n.start, 0);
        assert_eq!(n.end, u32::MAX);
        assert_eq!(n.key, 0);
        assert_eq!(n.vel, 127);
        let r = Note::from_row6([i64::MIN, -1, 999, 300, -16, i64::MAX]).row6();
        assert_eq!(r, [0, 0, 255, 127, 0, u32::MAX as i64]);
        // Slots cap at 254
        assert_eq!(Note::from_row6([0, 1, 60, 100, 255, 0]).slot, 254);
        assert_eq!(
            Note::row6(Note::from_row6([0, 1, 60, 100, 254, 0])),
            [0, 1, 60, 100, 254, 0]
        );
    }
}

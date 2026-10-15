//! Bounded HID input-descriptor reader for digitizer finger collections (Windows precision
//! touchpads). Item and usage meanings come from the public HID specification (Device Class
//! Definition for HID 1.11, §6.2.2) and HID Usage Tables, not a vendor implementation.

/// A usage: its page in the high 16 bits, its id in the low 16 (HID's "extended usage").
const fn usage(page: u32, id: u32) -> u32 {
    (page << 16) | id
}

/// Usage pages.
const GENERIC_DESKTOP: u32 = 0x01;
const DIGITIZER: u32 = 0x0d;

/// The usages a touchpad finger is read from.
const FINGER: u32 = usage(DIGITIZER, 0x22);
const TIP_SWITCH: u32 = usage(DIGITIZER, 0x42);
const CONFIDENCE: u32 = usage(DIGITIZER, 0x47);
const CONTACT_ID: u32 = usage(DIGITIZER, 0x51);
const CONTACT_COUNT: u32 = usage(DIGITIZER, 0x54);
const X: u32 = usage(GENERIC_DESKTOP, 0x30);
const Y: u32 = usage(GENERIC_DESKTOP, 0x31);
/// Fields worth keeping; everything else in a report is skipped by its bit offset.
const READ: [u32; 6] = [TIP_SWITCH, CONFIDENCE, CONTACT_ID, CONTACT_COUNT, X, Y];
/// What a finger needs to be drawn with.
const REQUIRED: [u32; 3] = [TIP_SWITCH, X, Y];

/// A short item's prefix byte: bits 0–1 the data size, 2–3 the type, 4–7 the tag (§6.2.2.2).
const LONG_ITEM: u8 = 0xfe;
#[derive(Clone, Copy, PartialEq)]
enum ItemType {
    Main,
    Global,
    Local,
    Reserved,
}
/// Main item tags.
const INPUT: u8 = 0x8;
const COLLECTION: u8 = 0xa;
const END_COLLECTION: u8 = 0xc;
/// Global item tags.
const USAGE_PAGE: u8 = 0x0;
const LOGICAL_MINIMUM: u8 = 0x1;
const LOGICAL_MAXIMUM: u8 = 0x2;
const REPORT_SIZE: u8 = 0x7;
const REPORT_ID: u8 = 0x8;
const REPORT_COUNT: u8 = 0x9;
const PUSH: u8 = 0xa;
const POP: u8 = 0xb;
/// Local item tags.
const USAGE: u8 = 0x0;
/// Input item data bits: bit 0 set is a constant (padding), bit 1 set a variable (one value per
/// usage, not an array of selectors).
const CONSTANT: u32 = 1 << 0;
const VARIABLE: u32 = 1 << 1;

/// Bounds against hostile descriptors and reports.
const MAX_DESCRIPTOR_BYTES: usize = 4096;
const MAX_PUSH_DEPTH: usize = 16;
const MAX_LOCAL_USAGES: usize = 256;
const MAX_COLLECTION_DEPTH: usize = 32;
const MAX_FINGERS: usize = 16;
const MAX_FIELD_BITS: usize = 32;
const MAX_REPORT_COUNT: usize = 256;
const MAX_REPORT_BITS: usize = 32768;
const MAX_FIELDS: usize = 128;

/// The global item state (§6.2.2.7), saved and restored by Push and Pop.
#[derive(Clone, Copy, Default)]
struct Global {
    page: u32,
    min: i64,
    max: i64,
    size: usize,
    count: usize,
    report: u8,
}
/// One value in an input report: which report and finger it belongs to, and where its bits are.
#[derive(Clone, Copy)]
struct Field {
    report: u8,
    /// 1-based finger collection; 0 is outside any finger (the contact count).
    finger: usize,
    usage: u32,
    bit: usize,
    size: usize,
    min: i64,
    max: i64,
}
pub struct Descriptor {
    fields: Vec<Field>,
    /// Reports start with a report id byte (the descriptor declared one).
    numbered: bool,
}
impl Descriptor {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_DESCRIPTOR_BYTES {
            return None;
        }
        let mut global = Global::default();
        let mut pushed = Vec::new();
        let mut collections = Vec::new();
        let mut usages = Vec::new();
        let mut bit_offsets = [0usize; 256];
        let mut fields = Vec::new();
        let mut finger = 0usize;
        let mut next_finger = 1usize;
        let mut numbered = false;
        let mut i = 0usize;
        while i < bytes.len() {
            let prefix = *bytes.get(i)?;
            i += 1;
            if prefix == LONG_ITEM {
                return None;
            }
            let size = match prefix & 0b11 {
                3 => 4,
                n => n as usize,
            };
            let data = bytes.get(i..i.checked_add(size)?)?;
            i += size;
            let kind = match (prefix >> 2) & 0b11 {
                0 => ItemType::Main,
                1 => ItemType::Global,
                2 => ItemType::Local,
                _ => ItemType::Reserved,
            };
            let tag = prefix >> 4;
            // Little-endian data, read as unsigned and as two's-complement signed.
            let unsigned = data.iter().enumerate().fold(0u32, |v, (j, b)| v | ((*b as u32) << (8 * j)));
            let negative = data.last().is_some_and(|b| b & 0x80 != 0);
            let signed = if negative { unsigned as i64 - (1i64 << (size * 8)) } else { unsigned as i64 };
            match (kind, tag) {
                (ItemType::Global, USAGE_PAGE) => global.page = unsigned,
                (ItemType::Global, LOGICAL_MINIMUM) => global.min = signed,
                // The maximum is signed only when the minimum is (§6.2.2.7).
                (ItemType::Global, LOGICAL_MAXIMUM) => global.max = if global.min < 0 { signed } else { unsigned as i64 },
                (ItemType::Global, REPORT_SIZE) => global.size = unsigned as usize,
                (ItemType::Global, REPORT_ID) => {
                    global.report = u8::try_from(unsigned).ok()?;
                    numbered = true;
                }
                (ItemType::Global, REPORT_COUNT) => global.count = unsigned as usize,
                (ItemType::Global, PUSH) => {
                    if pushed.len() >= MAX_PUSH_DEPTH {
                        return None;
                    }
                    pushed.push(global);
                }
                (ItemType::Global, POP) => global = pushed.pop()?,
                (ItemType::Local, USAGE) => {
                    if usages.len() >= MAX_LOCAL_USAGES {
                        return None;
                    }
                    // A 4-byte usage carries its own page; shorter ones use the current page.
                    usages.push(if size == 4 { unsigned } else { usage(global.page, unsigned) });
                }
                (ItemType::Main, COLLECTION) => {
                    if collections.len() >= MAX_COLLECTION_DEPTH {
                        return None;
                    }
                    collections.push(finger);
                    if usages.last() == Some(&FINGER) {
                        finger = next_finger;
                        next_finger += 1;
                        if next_finger > MAX_FINGERS + 1 {
                            return None;
                        }
                    }
                    usages.clear();
                }
                (ItemType::Main, END_COLLECTION) => {
                    finger = collections.pop()?;
                    usages.clear();
                }
                (ItemType::Main, INPUT) => {
                    if global.size > MAX_FIELD_BITS || global.count > MAX_REPORT_COUNT {
                        return None;
                    }
                    let offset = bit_offsets.get_mut(global.report as usize)?;
                    if unsigned & (CONSTANT | VARIABLE) == VARIABLE {
                        for j in 0..global.count {
                            // Fewer usages than values: the last usage repeats (§6.2.2.8).
                            if let Some(&usage) = usages.get(j).or_else(|| usages.last())
                                && READ.contains(&usage)
                            {
                                fields.push(Field {
                                    report: global.report,
                                    finger,
                                    usage,
                                    bit: offset.checked_add(j.checked_mul(global.size)?)?,
                                    size: global.size,
                                    min: global.min,
                                    max: global.max,
                                });
                            }
                        }
                    }
                    *offset = offset.checked_add(global.size.checked_mul(global.count)?)?;
                    if *offset > MAX_REPORT_BITS || fields.len() > MAX_FIELDS {
                        return None;
                    }
                    usages.clear();
                }
                // Output and Feature items: other reports, nothing to read here.
                (ItemType::Main, _) => usages.clear(),
                _ => {}
            }
        }
        let descriptor = Self { fields, numbered };
        let drawable =
            (1..next_finger).any(|finger| REQUIRED.iter().all(|usage| descriptor.fields.iter().any(|f| f.finger == finger && f.usage == *usage)));
        drawable.then_some(descriptor)
    }

    /// Decodes one input report. `None` when it isn't one of the touchpad's reports.
    pub fn packet(&self, report: &[u8]) -> Option<Packet> {
        let (id, data) = if self.numbered { (*report.first()?, report.get(1..)?) } else { (0, report) };
        let mut fingers = [FingerState::default(); MAX_FINGERS + 1];
        let mut identities = [None; MAX_FINGERS + 1];
        let mut count = None;
        let mut matched = false;
        for f in self.fields.iter().filter(|f| f.report == id) {
            matched = true;
            let mut raw = 0u32;
            for bit in 0..f.size {
                let index = f.bit.checked_add(bit)?;
                raw |= (((data.get(index / 8)? >> (index % 8)) & 1) as u32) << bit;
            }
            let negative = f.min < 0 && f.size > 0 && raw & (1u32 << (f.size - 1)) != 0;
            let value = if negative { raw as i64 - (1i64 << f.size) } else { raw as i64 };
            let state = fingers.get_mut(f.finger)?;
            match f.usage {
                CONTACT_COUNT => count = Some(usize::try_from(value).ok()?),
                CONTACT_ID => *identities.get_mut(f.finger)? = Some(u32::try_from(value).ok()?),
                TIP_SWITCH => state.tip = Some(value != 0),
                CONFIDENCE => state.confident = Some(value != 0),
                X | Y if f.max > f.min => {
                    let normalized = Some(((value - f.min) as f64 / (f.max - f.min) as f64).clamp(0.0, 1.0) as f32);
                    if f.usage == X {
                        state.x = normalized;
                    } else {
                        state.y = normalized;
                    }
                }
                _ => {}
            }
        }
        if !matched {
            return None;
        }
        let contacts = fingers
            .iter()
            .enumerate()
            .filter_map(|(i, finger)| {
                let tip = finger.tip?;
                // A palm (not confident) or a lifted finger is still a contact, with no point.
                let point = (tip && finger.confident.unwrap_or(true)).then_some([finger.x?, finger.y?]);
                Some(Contact { id: identities.get(i).copied().flatten().unwrap_or(i as u32), point })
            })
            .collect::<Vec<_>>();
        let count = count.unwrap_or(contacts.len());
        if count > MAX_FINGERS {
            return None;
        }
        Some(Packet { count, contacts })
    }
}

/// What one report said about one finger.
#[derive(Clone, Copy, Default)]
struct FingerState {
    tip: Option<bool>,
    x: Option<f32>,
    y: Option<f32>,
    confident: Option<bool>,
}
#[derive(Clone, Copy)]
struct Contact {
    id: u32,
    point: Option<[f32; 2]>,
}
pub struct Packet {
    /// The contact count of the frame this report starts, or 0 for a continuation report.
    count: usize,
    contacts: Vec<Contact>,
}
/// Precision touchpads can distribute one physical frame across several HID reports ("hybrid
/// mode"): the first carries the frame's contact count, the rest only contacts.
#[derive(Default)]
pub struct Assembler {
    expected: usize,
    pending: Vec<Contact>,
}
impl Assembler {
    pub fn push(&mut self, packet: Packet) -> Option<Vec<[f32; 2]>> {
        if packet.count > 0 {
            self.expected = packet.count;
            self.pending.clear();
        }
        if self.expected == 0 {
            return Some(Vec::new());
        }
        let remaining = self.expected.saturating_sub(self.pending.len());
        for contact in packet.contacts.into_iter().take(remaining) {
            if !self.pending.iter().any(|before| before.id == contact.id) {
                self.pending.push(contact);
            }
        }
        if self.pending.len() != self.expected {
            return None;
        }
        self.expected = 0;
        Some(std::mem::take(&mut self.pending).iter().filter_map(|c| c.point).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds descriptors item by item, so the tests read like the descriptor they describe.
    #[derive(Default)]
    struct Builder(Vec<u8>);
    impl Builder {
        fn item(mut self, kind: u8, tag: u8, data: &[u8]) -> Self {
            let size = match data.len() {
                4 => 3,
                n => n as u8,
            };
            self.0.push((tag << 4) | (kind << 2) | size);
            self.0.extend_from_slice(data);
            self
        }
        fn usage_page(self, page: u32) -> Self {
            self.item(1, USAGE_PAGE, &[page as u8])
        }
        fn usage(self, usage: u32) -> Self {
            self.item(2, USAGE, &[usage as u8])
        }
        fn logical(self, min: u8, max: &[u8]) -> Self {
            self.item(1, LOGICAL_MINIMUM, &[min]).item(1, LOGICAL_MAXIMUM, max)
        }
        fn report_id(self, id: u8) -> Self {
            self.item(1, REPORT_ID, &[id])
        }
        fn report_size(self, bits: u8) -> Self {
            self.item(1, REPORT_SIZE, &[bits])
        }
        fn report_count(self, count: u8) -> Self {
            self.item(1, REPORT_COUNT, &[count])
        }
        fn input_variable(self) -> Self {
            self.item(0, INPUT, &[VARIABLE as u8])
        }
        fn input_padding(self) -> Self {
            self.item(0, INPUT, &[CONSTANT as u8])
        }
        fn finger(self) -> Self {
            self.usage_page(DIGITIZER).usage(FINGER).item(0, COLLECTION, &[2])
        }
        fn end_collection(self) -> Self {
            self.item(0, END_COLLECTION, &[])
        }
    }

    /// Synthetic descriptors, not captured hardware or vendor fixtures.
    ///
    /// One finger: a tip bit (padded to a byte), then 8-bit X and Y.
    fn single_finger() -> Vec<u8> {
        Builder::default()
            .finger()
            .usage(TIP_SWITCH)
            .logical(0, &[1])
            .report_size(1)
            .report_count(1)
            .input_variable()
            .report_size(7)
            .input_padding()
            .usage_page(GENERIC_DESKTOP)
            .usage(X)
            .usage(Y)
            .item(1, LOGICAL_MAXIMUM, &[255, 0])
            .report_size(8)
            .report_count(2)
            .input_variable()
            .end_collection()
            .0
    }

    /// Report 7, one finger per report (hybrid mode): tip and confidence bits (padded to a byte),
    /// an 8-bit contact id, 8-bit X and Y; then the frame's 8-bit contact count outside the finger.
    fn hybrid() -> Vec<u8> {
        Builder::default()
            .report_id(7)
            .finger()
            .usage(TIP_SWITCH)
            .usage(CONFIDENCE)
            .logical(0, &[1])
            .report_size(1)
            .report_count(2)
            .input_variable()
            .report_size(6)
            .report_count(1)
            .input_padding()
            .usage(CONTACT_ID)
            .item(1, LOGICAL_MAXIMUM, &[255, 0])
            .report_size(8)
            .input_variable()
            .usage_page(GENERIC_DESKTOP)
            .usage(X)
            .usage(Y)
            .report_count(2)
            .input_variable()
            .end_collection()
            .usage_page(DIGITIZER)
            .usage(CONTACT_COUNT)
            .report_count(1)
            .input_variable()
            .0
    }

    #[test]
    fn physical_contacts_and_release_are_decoded() {
        let d = Descriptor::parse(&single_finger()).unwrap();
        let p = Assembler::default().push(d.packet(&[1, 128, 64]).unwrap()).unwrap();
        assert_eq!(p.len(), 1);
        assert!((p[0][0] - 128.0 / 255.0).abs() < 0.001);
        assert!(Assembler::default().push(d.packet(&[0, 128, 64]).unwrap()).unwrap().is_empty());
        assert!(d.packet(&[1]).is_none());
    }
    #[test]
    fn malformed_and_hostile_descriptors_are_rejected() {
        assert!(Descriptor::parse(&[LONG_ITEM]).is_none());
        // A report count of 65,535.
        assert!(Descriptor::parse(&Builder::default().item(1, REPORT_COUNT, &[255, 255]).input_variable().0).is_none());
        for n in 0..200 {
            let bytes = vec![0xff; n];
            assert!(Descriptor::parse(&bytes).is_none());
        }
    }
    #[test]
    fn hybrid_reports_are_combined_before_two_finger_pan() {
        let mut assembler = Assembler::default();
        assert!(assembler.push(Packet { count: 2, contacts: vec![Contact { id: 7, point: Some([0.2, 0.3]) }] }).is_none());
        let points = assembler.push(Packet { count: 0, contacts: vec![Contact { id: 19, point: Some([0.7, 0.8]) }] }).unwrap();
        assert_eq!(points, vec![[0.2, 0.3], [0.7, 0.8]]);
        assert!(assembler.push(Packet { count: 0, contacts: Vec::new() }).unwrap().is_empty());
    }
    #[test]
    fn report_ids_contact_ids_counts_and_confidence_are_decoded() {
        let d = Descriptor::parse(&hybrid()).unwrap();
        let mut assembler = Assembler::default();
        assert!(assembler.push(d.packet(&[7, 3, 19, 128, 64, 2]).unwrap()).is_none());
        let points = assembler.push(d.packet(&[7, 3, 7, 64, 128, 0]).unwrap()).unwrap();
        assert_eq!(points.len(), 2);
        assert!((points[0][0] - 128.0 / 255.0).abs() < 0.001);
        assert!(assembler.push(d.packet(&[7, 1, 7, 128, 64, 1]).unwrap()).unwrap().is_empty());
        assert!(d.packet(&[8, 3, 7, 64, 128, 1]).is_none());
    }
}

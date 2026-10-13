//! Bounded HID input-descriptor reader for digitizer finger collections.
//! Usage meanings come from the public HID specification, not a vendor implementation.
#[derive(Clone, Copy, Default)]
struct Global {
    page: u32,
    min: i64,
    max: i64,
    size: usize,
    count: usize,
    report: u8,
}
#[derive(Clone, Copy)]
struct Field {
    report: u8,
    finger: usize,
    usage: u32,
    bit: usize,
    size: usize,
    min: i64,
    max: i64,
}
pub struct Descriptor {
    fields: Vec<Field>,
    numbered: bool,
}
impl Descriptor {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > 4096 {
            return None;
        }
        let mut global = Global::default();
        let mut stack = Vec::new();
        let mut collections = Vec::new();
        let mut usages = Vec::new();
        let mut offsets = [0usize; 256];
        let mut fields = Vec::new();
        let mut finger = 0usize;
        let mut next_finger = 1usize;
        let mut i = 0usize;
        let mut numbered = false;
        while i < bytes.len() {
            let tag = *bytes.get(i)?;
            i += 1;
            if tag == 0xfe {
                return None;
            }
            let n = match tag & 3 {
                3 => 4,
                v => v as usize,
            };
            let data = bytes.get(i..i.checked_add(n)?)?;
            i += n;
            let unsigned = data.iter().enumerate().fold(0u32, |v, (j, b)| v | ((*b as u32) << (8 * j)));
            let signed = if n > 0 && data.last().is_some_and(|b| b & 128 != 0) { unsigned as i64 - (1i64 << (n * 8)) } else { unsigned as i64 };
            match (tag >> 2 & 3, tag >> 4) {
                (1, 0) => global.page = unsigned,
                (1, 1) => global.min = signed,
                (1, 2) => global.max = if global.min < 0 { signed } else { unsigned as i64 },
                (1, 7) => global.size = unsigned as usize,
                (1, 8) => {
                    global.report = u8::try_from(unsigned).ok()?;
                    numbered = true;
                }
                (1, 9) => global.count = unsigned as usize,
                (1, 10) => {
                    if stack.len() >= 16 {
                        return None;
                    }
                    stack.push(global);
                }
                (1, 11) => global = stack.pop()?,
                (2, 0) => {
                    if usages.len() >= 256 {
                        return None;
                    }
                    usages.push(if n == 4 { unsigned } else { (global.page << 16) | unsigned });
                }
                (0, 10) => {
                    if collections.len() >= 32 {
                        return None;
                    }
                    collections.push(finger);
                    if usages.last() == Some(&0x000d0022) {
                        finger = next_finger;
                        next_finger += 1;
                        if next_finger > 17 {
                            return None;
                        }
                    }
                    usages.clear();
                }
                (0, 12) => {
                    finger = collections.pop()?;
                    usages.clear();
                }
                (0, 8) => {
                    if global.size > 32 || global.count > 256 {
                        return None;
                    }
                    let offset = offsets.get_mut(global.report as usize)?;
                    if unsigned & 3 == 2 {
                        for j in 0..global.count {
                            if let Some(&usage) = usages.get(j).or_else(|| usages.last())
                                && [0x000d0042, 0x000d0047, 0x000d0051, 0x000d0054, 0x00010030, 0x00010031].contains(&usage)
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
                    if *offset > 32768 || fields.len() > 128 {
                        return None;
                    }
                    usages.clear();
                }
                (0, _) => usages.clear(),
                _ => {}
            }
        }
        let descriptor = Self { fields, numbered };
        let complete = (1..next_finger).any(|finger| {
            [0x000d0042, 0x00010030, 0x00010031].iter().all(|usage| descriptor.fields.iter().any(|f| f.finger == finger && f.usage == *usage))
        });
        complete.then_some(descriptor)
    }
    pub fn packet(&self, report: &[u8]) -> Option<Packet> {
        let (id, data) = if self.numbered { (*report.first()?, report.get(1..)?) } else { (0, report) };
        let mut fingers = [[None; 4]; 17];
        let mut identities = [None; 17];
        let mut count = None;
        let mut matched = false;
        for f in self.fields.iter().filter(|f| f.report == id) {
            matched = true;
            let mut value = 0u32;
            for bit in 0..f.size {
                let index = f.bit.checked_add(bit)?;
                value |= (((data.get(index / 8)? >> (index % 8)) & 1) as u32) << bit;
            }
            let value = if f.min < 0 && f.size > 0 && value & (1u32 << (f.size - 1)) != 0 { value as i64 - (1i64 << f.size) } else { value as i64 };
            if f.usage == 0x000d0054 {
                count = Some(usize::try_from(value).ok()?);
                continue;
            }
            if f.usage == 0x000d0051 {
                *identities.get_mut(f.finger)? = Some(u32::try_from(value).ok()?);
                continue;
            }
            let point = fingers.get_mut(f.finger)?;
            match f.usage {
                0x000d0047 => point[3] = Some(if value != 0 { 1.0 } else { 0.0 }),
                0x000d0042 => point[0] = Some(if value != 0 { 1.0 } else { 0.0 }),
                0x00010030 | 0x00010031 if f.max > f.min => {
                    point[if f.usage == 0x00010030 { 1 } else { 2 }] = Some(((value - f.min) as f64 / (f.max - f.min) as f64).clamp(0.0, 1.0) as f32)
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
            .filter_map(|(i, v)| {
                let tip = v[0]?;
                let point = (tip == 1.0 && v[3].unwrap_or(1.0) == 1.0).then_some([v[1]?, v[2]?]);
                Some(Contact { id: identities.get(i).copied().flatten().unwrap_or(i as u32), point })
            })
            .collect::<Vec<_>>();
        let count = count.unwrap_or(contacts.len());
        if count > 16 {
            return None;
        }
        Some(Packet { count, contacts })
    }
}
#[derive(Clone, Copy)]
struct Contact {
    id: u32,
    point: Option<[f32; 2]>,
}
pub struct Packet {
    count: usize,
    contacts: Vec<Contact>,
}
/// Precision touchpads can distribute one physical frame across several HID reports.
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
    #[test]
    fn physical_contacts_and_release_are_decoded() {
        // Original minimal synthetic descriptor: one finger, packed tip bit and 8-bit coordinates.
        let d = Descriptor::parse(&[
            0x05, 13, 0x09, 34, 0xa1, 2, 0x09, 66, 0x15, 0, 0x25, 1, 0x75, 1, 0x95, 1, 0x81, 2, 0x75, 7, 0x81, 1, 0x05, 1, 0x09, 48, 0x09, 49, 0x26,
            255, 0, 0x75, 8, 0x95, 2, 0x81, 2, 0xc0,
        ])
        .unwrap();
        let p = Assembler::default().push(d.packet(&[1, 128, 64]).unwrap()).unwrap();
        assert_eq!(p.len(), 1);
        assert!((p[0][0] - 128.0 / 255.0).abs() < 0.001);
        assert!(Assembler::default().push(d.packet(&[0, 128, 64]).unwrap()).unwrap().is_empty());
        assert!(d.packet(&[1]).is_none());
    }
    #[test]
    fn malformed_and_hostile_descriptors_are_rejected() {
        assert!(Descriptor::parse(&[0xfe]).is_none());
        assert!(Descriptor::parse(&[0x96, 255, 255, 0x81, 2]).is_none());
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
        // Original synthetic hybrid descriptor, not a captured hardware/vendor fixture.
        let d = Descriptor::parse(&[
            0x85, 7, 0x05, 13, 0x09, 34, 0xa1, 2, 0x09, 66, 0x09, 71, 0x15, 0, 0x25, 1, 0x75, 1, 0x95, 2, 0x81, 2, 0x75, 6, 0x95, 1, 0x81, 1, 0x09,
            81, 0x26, 255, 0, 0x75, 8, 0x81, 2, 0x05, 1, 0x09, 48, 0x09, 49, 0x95, 2, 0x81, 2, 0xc0, 0x05, 13, 0x09, 84, 0x95, 1, 0x81, 2,
        ])
        .unwrap();
        let mut assembler = Assembler::default();
        assert!(assembler.push(d.packet(&[7, 3, 19, 128, 64, 2]).unwrap()).is_none());
        let points = assembler.push(d.packet(&[7, 3, 7, 64, 128, 0]).unwrap()).unwrap();
        assert_eq!(points.len(), 2);
        assert!((points[0][0] - 128.0 / 255.0).abs() < 0.001);
        assert!(assembler.push(d.packet(&[7, 1, 7, 128, 64, 1]).unwrap()).unwrap().is_empty());
        assert!(d.packet(&[8, 3, 7, 64, 128, 1]).is_none());
    }
}

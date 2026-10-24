//! A property list read as a tree of values, binary (`bplist00`) or XML: what the Finder keeps
//! its list of tags in. Only what that needs is understood (dictionaries, arrays, strings,
//! integers); any other kind of value reads as [`Value::Other`]. Never panics, never recurses
//! without a bound: the file is the system's, but it is not trusted for that.

use super::bplist::read_uint;

/// How deep a list may nest, and how many objects it may hold, before it is refused.
const MAX_DEPTH: usize = 24;
const MAX_OBJECTS: u32 = 200_000;

/// One value of a property list.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Dict(Vec<(String, Value)>),
    Array(Vec<Value>),
    Str(String),
    Int(i64),
    Other,
}

impl Value {
    /// The value under `key`, when this is a dictionary that has it.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(d) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The first value under `key` in this dictionary or in the ones inside it, looking
    /// `depth` levels down: the Finder moved its list of tags between versions of the system,
    /// but always kept it under the same name.
    pub fn find(&self, key: &str, depth: usize) -> Option<&Value> {
        if let Some(v) = self.get(key) {
            return Some(v);
        }
        match self {
            Value::Dict(d) if depth > 0 => d.iter().find_map(|(_, v)| v.find(key, depth - 1)),
            _ => None,
        }
    }
}

/// The top value of a property list; `None` when it is not one this reads.
pub fn decode(bytes: &[u8]) -> Option<Value> {
    if bytes.starts_with(b"bplist00") {
        let (bin, top) = Binary::new(bytes)?;
        bin.value(top, 0)
    } else {
        Xml {
            rest: std::str::from_utf8(bytes).ok()?,
        }
        .document()
    }
}

// ----------------------------------------------------------------------------------- binary

struct Binary<'a> {
    b: &'a [u8],
    offset_size: usize,
    ref_size: usize,
    count: usize,
    table_at: usize,
    budget: std::cell::Cell<u32>,
}

impl<'a> Binary<'a> {
    /// The reader, and the number of the top object.
    fn new(b: &'a [u8]) -> Option<(Binary<'a>, usize)> {
        if b.len() < 8 + 32 {
            return None;
        }
        let t = &b[b.len() - 32..];
        let bin = Binary {
            b,
            offset_size: t[6] as usize,
            ref_size: t[7] as usize,
            count: read_uint(&t[8..16])? as usize,
            table_at: read_uint(&t[24..32])? as usize,
            budget: std::cell::Cell::new(MAX_OBJECTS),
        };
        let top = read_uint(&t[16..24])? as usize;
        let sizes_ok = (1..=8).contains(&bin.offset_size) && (1..=8).contains(&bin.ref_size);
        let table_end = bin
            .table_at
            .checked_add(bin.count.checked_mul(bin.offset_size)?)?;
        (sizes_ok && top < bin.count && table_end <= b.len() - 32).then_some((bin, top))
    }

    fn bytes(&self, at: usize, n: usize) -> Option<&'a [u8]> {
        self.b.get(at..at.checked_add(n)?)
    }

    fn offset_of(&self, i: usize) -> Option<usize> {
        let at = self
            .table_at
            .checked_add(i.checked_mul(self.offset_size)?)?;
        read_uint(self.bytes(at, self.offset_size)?).map(|o| o as usize)
    }

    /// The length an object says it has, and where its content starts: the low nibble of the
    /// marker, or an integer object after it when that is 15.
    fn length(&self, at: usize) -> Option<(usize, usize)> {
        let nibble = (*self.b.get(at)? & 0x0F) as usize;
        if nibble < 15 {
            return Some((nibble, at + 1));
        }
        let int = *self.b.get(at + 1)?;
        if int & 0xF0 != 0x10 || int & 0x0F > 3 {
            return None;
        }
        let size = 1usize << (int & 0x0F);
        let n = read_uint(self.bytes(at + 2, size)?)? as usize;
        Some((n, at + 2 + size))
    }

    fn reference(&self, refs_at: usize, i: usize) -> Option<usize> {
        let at = refs_at.checked_add(i.checked_mul(self.ref_size)?)?;
        read_uint(self.bytes(at, self.ref_size)?).map(|r| r as usize)
    }

    fn value(&self, i: usize, depth: usize) -> Option<Value> {
        let left = self.budget.get().checked_sub(1)?;
        self.budget.set(left);
        if i >= self.count || depth > MAX_DEPTH {
            return None;
        }
        let at = self.offset_of(i)?;
        let marker = *self.b.get(at)?;
        match marker >> 4 {
            0x1 => {
                let size = 1usize << (marker & 0x0F);
                let n = read_uint(self.bytes(at + 1, size).filter(|_| size <= 8)?)?;
                Some(Value::Int(n as i64))
            }
            0x5 | 0x7 => {
                let (len, data) = self.length(at)?;
                let s = String::from_utf8(self.bytes(data, len)?.to_vec()).ok()?;
                Some(Value::Str(s))
            }
            0x6 => {
                let (len, data) = self.length(at)?;
                let raw = self.bytes(data, len.checked_mul(2)?)?;
                let units: Vec<u16> = raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_be_bytes(*c))
                    .collect();
                Some(Value::Str(String::from_utf16(&units).ok()?))
            }
            0xA => self.container(at, depth, false),
            0xD => self.container(at, depth, true),
            _ => Some(Value::Other),
        }
    }

    /// An array, or a dictionary: its keys come first among the references, then its values.
    fn container(&self, at: usize, depth: usize, dict: bool) -> Option<Value> {
        let (n, refs_at) = self.length(at)?;
        if n > self.count {
            return None;
        }
        let item = |i: usize| self.value(self.reference(refs_at, i)?, depth + 1);
        if !dict {
            return (0..n)
                .map(item)
                .collect::<Option<Vec<_>>>()
                .map(Value::Array);
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let Value::Str(key) = item(i)? else {
                return None;
            };
            out.push((key, item(n + i)?));
        }
        Some(Value::Dict(out))
    }
}

// --------------------------------------------------------------------------------------- XML

/// The text of an XML property list, taken from the front as it is read.
struct Xml<'a> {
    rest: &'a str,
}

impl<'a> Xml<'a> {
    fn document(mut self) -> Option<Value> {
        let at = self.rest.find("<plist")?;
        self.rest = &self.rest[at..];
        self.rest = &self.rest[self.rest.find('>')? + 1..];
        self.element(0)
    }

    fn skip_space(&mut self) {
        self.rest = self.rest.trim_start();
    }

    /// Take `s` off the front if it is there.
    fn eat(&mut self, s: &str) -> bool {
        self.skip_space();
        match self.rest.strip_prefix(s) {
            Some(r) => {
                self.rest = r;
                true
            }
            None => false,
        }
    }

    /// The text up to `end`, which is taken too.
    fn text_until(&mut self, end: &str) -> Option<&'a str> {
        let at = self.rest.find(end)?;
        let text = &self.rest[..at];
        self.rest = &self.rest[at + end.len()..];
        Some(text)
    }

    fn element(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        if self.eat("<dict/>") {
            return Some(Value::Dict(Vec::new()));
        }
        if self.eat("<array/>") {
            return Some(Value::Array(Vec::new()));
        }
        if self.eat("<string/>") {
            return Some(Value::Str(String::new()));
        }
        if self.eat("<dict>") {
            return self.dict(depth);
        }
        if self.eat("<array>") {
            return self.array(depth);
        }
        if self.eat("<string>") {
            return Some(Value::Str(unescape(self.text_until("</string>")?)));
        }
        if self.eat("<integer>") {
            let n = self.text_until("</integer>")?.trim().parse().ok()?;
            return Some(Value::Int(n));
        }
        self.other()
    }

    /// `<true/>`, `<real>…</real>` and the like: read past, not understood.
    fn other(&mut self) -> Option<Value> {
        self.skip_space();
        let rest = self.rest.strip_prefix('<')?;
        let head = &rest[..rest.find('>')?];
        let name: String = head
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if name.is_empty() {
            return None;
        }
        if head.ends_with('/') {
            self.rest = &rest[head.len() + 1..];
        } else {
            self.text_until(&format!("</{name}>"))?;
        }
        Some(Value::Other)
    }

    fn array(&mut self, depth: usize) -> Option<Value> {
        let mut out = Vec::new();
        while !self.eat("</array>") {
            out.push(self.element(depth + 1)?);
        }
        Some(Value::Array(out))
    }

    fn dict(&mut self, depth: usize) -> Option<Value> {
        let mut out = Vec::new();
        while !self.eat("</dict>") {
            if !self.eat("<key>") {
                return None;
            }
            let key = unescape(self.text_until("</key>")?);
            out.push((key, self.element(depth + 1)?));
        }
        Some(Value::Dict(out))
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }

    fn dict(items: Vec<(&str, Value)>) -> Value {
        Value::Dict(items.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    fn list() -> Value {
        Value::Array(vec![s("a"), dict(vec![("x", Value::Int(300))])])
    }

    /// A dictionary in binary form, built by hand: offsets and references of one byte.
    fn binary() -> Vec<u8> {
        let mut objs: Vec<Vec<u8>> = vec![
            vec![0xD4, 1, 3, 5, 7, 2, 4, 6, 8],
            [vec![0x51], b"n".to_vec()].concat(),
            vec![
                0x65, 0x00, 0x43, 0x00, 0x69, 0x00, 0x74, 0x00, 0x74, 0x00,
                0xE0, // "Città", UTF-16
            ],
            [vec![0x51], b"l".to_vec()].concat(),
            vec![0x10, 2],
            [vec![0x52], b"ok".to_vec()].concat(),
            vec![0x09],
            [vec![0x54], b"list".to_vec()].concat(),
            vec![0xA2, 9, 10],
            [vec![0x51], b"a".to_vec()].concat(),
            vec![0xD1, 11, 12],
            [vec![0x51], b"x".to_vec()].concat(),
            vec![0x11, 0x01, 0x2C],
        ];
        let mut out = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        for o in objs.drain(..) {
            offsets.push(out.len() as u8);
            out.extend_from_slice(&o);
        }
        let table_at = out.len() as u64;
        out.extend_from_slice(&offsets);
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&[1, 1]);
        out.extend_from_slice(&(offsets.len() as u64).to_be_bytes());
        out.extend_from_slice(&0u64.to_be_bytes());
        out.extend_from_slice(&table_at.to_be_bytes());
        out
    }

    #[test]
    fn a_binary_list_is_read_as_a_tree() {
        let got = decode(&binary()).unwrap();
        assert_eq!(got.get("n"), Some(&s("Città")));
        assert_eq!(got.get("l").and_then(Value::as_int), Some(2));
        assert_eq!(got.get("ok"), Some(&Value::Other));
        assert_eq!(got.get("list"), Some(&list()));
    }

    #[test]
    fn an_xml_list_reads_as_a_tree() {
        let x = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>n</key><string>Citt&amp;à</string>
  <key>l</key><integer>2</integer>
  <key>ok</key><true/>
  <key>list</key><array><string>a</string><dict><key>x</key><integer>300</integer></dict></array>
</dict></plist>"#;
        let want = dict(vec![
            ("n", s("Citt&à")),
            ("l", Value::Int(2)),
            ("ok", Value::Other),
            ("list", list()),
        ]);
        assert_eq!(decode(x.as_bytes()), Some(want));
        assert_eq!(decode(b"<plist><dict/></plist>"), Some(Value::Dict(vec![])));
    }

    #[test]
    fn a_key_is_found_a_few_levels_down() {
        let v = dict(vec![(
            "values",
            dict(vec![("d", dict(vec![("tags", Value::Int(1))]))]),
        )]);
        assert_eq!(v.find("tags", 2), Some(&Value::Int(1)));
        assert_eq!(v.find("tags", 1), None);
        assert_eq!(v.find("none", 5), None);
    }

    #[test]
    fn what_is_not_a_list_is_refused_and_never_panics() {
        assert!(decode(b"").is_none());
        assert!(decode(b"bplist00").is_none());
        assert!(decode(b"garbage").is_none());
        assert!(decode(b"<plist><dict><key>a</key></dict></plist>").is_none());
        let good = binary();
        for cut in 0..good.len() {
            let _ = decode(&good[..cut]);
        }
        // Every byte changed to every other: whatever comes out, nothing panics or loops.
        for at in 8..good.len() {
            let mut bad = good.clone();
            bad[at] = bad[at].wrapping_add(0x55);
            let _ = decode(&bad);
        }
    }

    #[test]
    fn a_list_that_contains_itself_is_refused() {
        // One array whose only item is itself.
        let mut b = b"bplist00".to_vec();
        b.extend_from_slice(&[0xA1, 0x00]);
        let table_at = b.len() as u64;
        b.push(8);
        b.extend_from_slice(&[0; 6]);
        b.extend_from_slice(&[1, 1]);
        b.extend_from_slice(&1u64.to_be_bytes());
        b.extend_from_slice(&0u64.to_be_bytes());
        b.extend_from_slice(&table_at.to_be_bytes());
        assert!(decode(&b).is_none());
    }
}

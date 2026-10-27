//! A minimal JDWP (Java Debug Wire Protocol) client: enough to find a game's
//! classes, list their live objects, read fields and change them while the
//! game runs. It never suspends the game.
//!
//! Packets: `length u32 | id u32 | flags u8 | (command set u8, command u8)`
//! for commands, or `(error code u16)` for replies (flags 0x80). All numbers
//! are big-endian. ID sizes come from the VM (IDSizes).

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct IdSizes {
    pub field: usize,
    pub method: usize,
    pub object: usize,
    pub reference_type: usize,
    pub frame: usize,
}

pub struct Jdwp {
    stream: TcpStream,
    next_id: u32,
    pub sizes: IdSizes,
}

/// JDWP error codes worth explaining.
fn error_name(code: u16) -> String {
    match code {
        20 => "That object no longer exists (the game removed it).".into(),
        21 => "That class isn't loaded in the game right now.".into(),
        25 => "That field doesn't belong to this object.".into(),
        34 => "The value has the wrong type for this field.".into(),
        99 => "The game's Java runtime doesn't support this.".into(),
        112 => "The game's Java runtime is shutting down.".into(),
        n => format!("The game's debugger interface returned error {n}."),
    }
}

/// Reads values out of a reply.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    sizes: IdSizes,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], sizes: IdSizes) -> Self {
        Reader { data, pos: 0, sizes }
    }

    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self.pos + n;
        let slice = self.data.get(self.pos..end).ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "short JDWP reply"))?;
        self.pos = end;
        Ok(slice)
    }

    pub fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    /// An ID of the given byte size.
    pub fn id(&mut self, size: usize) -> io::Result<u64> {
        Ok(self.take(size)?.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64))
    }

    pub fn object(&mut self) -> io::Result<u64> {
        let size = self.sizes.object;
        self.id(size)
    }

    pub fn reference_type(&mut self) -> io::Result<u64> {
        let size = self.sizes.reference_type;
        self.id(size)
    }

    pub fn field(&mut self) -> io::Result<u64> {
        let size = self.sizes.field;
        self.id(size)
    }

    pub fn string(&mut self) -> io::Result<String> {
        let len = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }

    /// A tagged value: (tag, value).
    pub fn value(&mut self) -> io::Result<(u8, Value)> {
        let tag = self.u8()?;
        Ok((tag, self.untagged(tag)?))
    }

    pub fn untagged(&mut self, tag: u8) -> io::Result<Value> {
        Ok(match tag {
            b'B' => Value::Int(self.u8()? as i8 as i64),
            b'C' => Value::Char(char::from_u32(self.u16()? as u32).unwrap_or('?')),
            b'S' => Value::Int(self.u16()? as i16 as i64),
            b'I' => Value::Int(self.u32()? as i32 as i64),
            b'J' => Value::Int(self.u64()? as i64),
            b'F' => Value::Float(f32::from_bits(self.u32()?) as f64),
            b'D' => Value::Float(f64::from_bits(self.u64()?)),
            b'Z' => Value::Bool(self.u8()? != 0),
            b'V' => Value::Void,
            _ => Value::Object(self.object()?),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    /// An object ID (0 = null); strings, arrays and objects.
    Object(u64),
    Void,
}

/// Builds command data.
pub struct Writer {
    pub data: Vec<u8>,
    sizes: IdSizes,
}

impl Writer {
    pub fn new(sizes: IdSizes) -> Self {
        Writer { data: Vec::new(), sizes }
    }
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.data.push(v);
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.data.extend_from_slice(&v.to_be_bytes());
        self
    }
    pub fn id(&mut self, v: u64, size: usize) -> &mut Self {
        self.data.extend_from_slice(&v.to_be_bytes()[8 - size..]);
        self
    }
    pub fn object(&mut self, v: u64) -> &mut Self {
        let size = self.sizes.object;
        self.id(v, size)
    }
    pub fn reference_type(&mut self, v: u64) -> &mut Self {
        let size = self.sizes.reference_type;
        self.id(v, size)
    }
    pub fn field(&mut self, v: u64) -> &mut Self {
        let size = self.sizes.field;
        self.id(v, size)
    }
    pub fn string(&mut self, s: &str) -> &mut Self {
        self.u32(s.len() as u32);
        self.data.extend_from_slice(s.as_bytes());
        self
    }
    /// An untagged primitive value for a field of signature `tag`.
    pub fn untagged(&mut self, tag: u8, v: &Value) -> io::Result<&mut Self> {
        let bad = || io::Error::new(io::ErrorKind::InvalidInput, "value doesn't match the field type");
        match (tag, v) {
            (b'B', Value::Int(n)) => self.data.push(*n as i8 as u8),
            (b'S', Value::Int(n)) => self.data.extend_from_slice(&(*n as i16).to_be_bytes()),
            (b'C', Value::Char(c)) => self.data.extend_from_slice(&(*c as u16).to_be_bytes()),
            (b'I', Value::Int(n)) => self.data.extend_from_slice(&(*n as i32).to_be_bytes()),
            (b'J', Value::Int(n)) => self.data.extend_from_slice(&n.to_be_bytes()),
            (b'F', Value::Float(f)) => self.data.extend_from_slice(&(*f as f32).to_bits().to_be_bytes()),
            (b'D', Value::Float(f)) => self.data.extend_from_slice(&f.to_bits().to_be_bytes()),
            (b'Z', Value::Bool(b)) => self.data.push(*b as u8),
            _ => return Err(bad()),
        }
        Ok(self)
    }
}

impl Jdwp {
    /// Connects to a forwarded JDWP port and does the handshake.
    pub fn connect(port: u16) -> io::Result<Jdwp> {
        let stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(5))?;
        stream.set_read_timeout(Some(Duration::from_secs(15)))?;
        stream.set_nodelay(true)?;
        let mut client = Jdwp {
            stream,
            next_id: 1,
            sizes: IdSizes { field: 8, method: 8, object: 8, reference_type: 8, frame: 8 },
        };
        const HANDSHAKE: &[u8] = b"JDWP-Handshake";
        client.stream.write_all(HANDSHAKE)?;
        let mut reply = [0u8; 14];
        client.stream.read_exact(&mut reply)?;
        if reply != HANDSHAKE {
            return Err(io::Error::other("The game didn't answer like a Java debugger."));
        }
        let data = client.command(1, 7, &[])?;
        let mut r = Reader::new(&data, client.sizes);
        client.sizes = IdSizes {
            field: r.u32()? as usize,
            method: r.u32()? as usize,
            object: r.u32()? as usize,
            reference_type: r.u32()? as usize,
            frame: r.u32()? as usize,
        };
        Ok(client)
    }

    pub fn writer(&self) -> Writer {
        Writer::new(self.sizes)
    }

    pub fn reader<'a>(&self, data: &'a [u8]) -> Reader<'a> {
        Reader::new(data, self.sizes)
    }

    /// Sends a command and waits for its reply; events the VM sends in
    /// between are skipped.
    pub fn command(&mut self, set: u8, cmd: u8, data: &[u8]) -> io::Result<Vec<u8>> {
        let id = self.next_id;
        self.next_id += 1;
        let mut packet = Vec::with_capacity(11 + data.len());
        packet.extend_from_slice(&((11 + data.len()) as u32).to_be_bytes());
        packet.extend_from_slice(&id.to_be_bytes());
        packet.push(0);
        packet.push(set);
        packet.push(cmd);
        packet.extend_from_slice(data);
        self.stream.write_all(&packet)?;
        loop {
            let mut header = [0u8; 11];
            self.stream.read_exact(&mut header)?;
            let len = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
            let reply_id = u32::from_be_bytes(header[4..8].try_into().unwrap());
            let mut body = vec![0u8; len.saturating_sub(11)];
            self.stream.read_exact(&mut body)?;
            if header[8] & 0x80 == 0 || reply_id != id {
                continue; // an event, or a reply to something else
            }
            let error = u16::from_be_bytes([header[9], header[10]]);
            if error != 0 {
                return Err(io::Error::other(error_name(error)));
            }
            return Ok(body);
        }
    }

    /// Detaches cleanly; the game keeps running.
    pub fn dispose(&mut self) {
        let _ = self.command(1, 6, &[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: IdSizes = IdSizes { field: 8, method: 8, object: 8, reference_type: 8, frame: 8 };

    #[test]
    fn reads_and_writes_values() {
        let mut w = Writer::new(SIZES);
        w.untagged(b'I', &Value::Int(-5)).unwrap();
        w.untagged(b'F', &Value::Float(2.5)).unwrap();
        w.untagged(b'D', &Value::Float(-0.125)).unwrap();
        w.untagged(b'Z', &Value::Bool(true)).unwrap();
        w.untagged(b'J', &Value::Int(1 << 40)).unwrap();
        w.untagged(b'B', &Value::Int(-1)).unwrap();
        let mut r = Reader::new(&w.data, SIZES);
        assert_eq!(r.untagged(b'I').unwrap(), Value::Int(-5));
        assert_eq!(r.untagged(b'F').unwrap(), Value::Float(2.5));
        assert_eq!(r.untagged(b'D').unwrap(), Value::Float(-0.125));
        assert_eq!(r.untagged(b'Z').unwrap(), Value::Bool(true));
        assert_eq!(r.untagged(b'J').unwrap(), Value::Int(1 << 40));
        assert_eq!(r.untagged(b'B').unwrap(), Value::Int(-1));
        assert!(Writer::new(SIZES).untagged(b'I', &Value::Bool(true)).is_err());
    }

    #[test]
    fn ids_follow_the_vm_sizes() {
        let sizes = IdSizes { object: 4, ..SIZES };
        let mut w = Writer::new(sizes);
        w.object(0x0102_0304);
        assert_eq!(w.data, [1, 2, 3, 4]);
        assert_eq!(Reader::new(&w.data, sizes).object().unwrap(), 0x0102_0304);
    }

    /// A fake VM: answers the handshake and IDSizes, sends an event first,
    /// then replies to one command with an error.
    #[test]
    fn talks_to_a_vm_and_skips_events() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut hs = [0u8; 14];
            s.read_exact(&mut hs).unwrap();
            s.write_all(&hs).unwrap();
            let read_packet = |s: &mut TcpStream| -> u32 {
                let mut h = [0u8; 11];
                s.read_exact(&mut h).unwrap();
                let len = u32::from_be_bytes(h[0..4].try_into().unwrap()) as usize;
                let mut body = vec![0u8; len - 11];
                s.read_exact(&mut body).unwrap();
                u32::from_be_bytes(h[4..8].try_into().unwrap())
            };
            let reply = |s: &mut TcpStream, id: u32, error: u16, body: &[u8]| {
                let mut p = ((11 + body.len()) as u32).to_be_bytes().to_vec();
                p.extend_from_slice(&id.to_be_bytes());
                p.push(0x80);
                p.extend_from_slice(&error.to_be_bytes());
                p.extend_from_slice(body);
                s.write_all(&p).unwrap();
            };
            let id = read_packet(&mut s);
            let sizes: Vec<u8> = [8u32, 8, 4, 8, 8].iter().flat_map(|v| v.to_be_bytes()).collect();
            reply(&mut s, id, 0, &sizes);
            let id = read_packet(&mut s);
            // An event (a command from the VM) arrives first.
            let mut event = 13u32.to_be_bytes().to_vec();
            event.extend_from_slice(&99u32.to_be_bytes());
            event.extend_from_slice(&[0, 64, 100, 0, 0]);
            s.write_all(&event).unwrap();
            reply(&mut s, id, 20, &[]);
        });
        let mut vm = Jdwp::connect(port).unwrap();
        assert_eq!(vm.sizes.object, 4);
        let err = vm.command(9, 2, &[]).unwrap_err();
        assert!(err.to_string().contains("no longer exists"), "{err}");
        server.join().unwrap();
    }
}

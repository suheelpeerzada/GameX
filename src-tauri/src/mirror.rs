//! Shows a device's screen inside GameX and sends touches back. The screen
//! comes from Android's own recorder (`screenrecord`, H.264), split here into
//! frames the UI decodes with WebCodecs. Touches go through one persistent
//! `adb shell` as `input` commands, so each tap doesn't start a process.

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Splits an H.264 Annex B byte stream into access units (one frame each,
/// with any SPS/PPS that came before it).
#[derive(Default)]
pub struct FrameSplitter {
    buffer: Vec<u8>,
    pending: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub struct Frame {
    pub key: bool,
    /// Annex B bytes, start codes included.
    pub data: Vec<u8>,
}

fn start_code_at(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i..i + 3) == Some(&[0, 0, 1]) {
        Some(3)
    } else if b.get(i..i + 4) == Some(&[0, 0, 0, 1]) {
        Some(4)
    } else {
        None
    }
}

impl FrameSplitter {
    /// Feeds bytes and returns the frames completed so far.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Frame> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        // NAL units are complete once the next start code has arrived.
        let starts: Vec<usize> = (0..self.buffer.len()).filter(|&i| {
            start_code_at(&self.buffer, i).is_some() && (i == 0 || self.buffer[i - 1] != 0 || start_code_at(&self.buffer, i - 1).is_none())
        }).collect();
        if starts.len() < 2 {
            return frames;
        }
        for pair in starts.windows(2) {
            let nal = &self.buffer[pair[0]..pair[1]];
            let code = start_code_at(nal, 0).unwrap_or(3);
            let kind = nal.get(code).map_or(0, |h| h & 0x1F);
            self.pending.extend_from_slice(nal);
            // A slice ends a frame (one slice per frame from screenrecord).
            if kind == 1 || kind == 5 {
                frames.push(Frame { key: kind == 5, data: std::mem::take(&mut self.pending) });
            }
        }
        let last = *starts.last().unwrap();
        self.buffer.drain(..last);
        frames
    }
}

/// `avc1.PPCCLL` from the stream's SPS, for the UI's decoder.
pub fn codec_string(frame: &[u8]) -> Option<String> {
    let mut i = 0;
    while i + 4 < frame.len() {
        if let Some(code) = start_code_at(frame, i) {
            if frame[i + code] & 0x1F == 7 {
                let sps = frame.get(i + code + 1..i + code + 4)?;
                return Some(format!("avc1.{:02X}{:02X}{:02X}", sps[0], sps[1], sps[2]));
            }
            i += code;
        } else {
            i += 1;
        }
    }
    None
}

/// What the UI is sent: decoder setup once, then frames.
pub enum Message {
    Setup { codec: String, width: u32, height: u32, device_width: u32, device_height: u32 },
    Frame(Frame),
    Ended(String),
}

pub struct Mirror {
    stop: Arc<AtomicBool>,
    recorder: Arc<Mutex<Option<Child>>>,
    shell: Mutex<Option<(Child, ChildStdin)>>,
    adb: std::path::PathBuf,
    serial: String,
}

/// The stream size: the device size scaled to fit `max_width`, even numbers.
pub fn stream_size(device: (u32, u32), max_width: u32) -> (u32, u32) {
    let (w, h) = device;
    if w <= max_width {
        return (w & !1, h & !1);
    }
    let scale = max_width as f64 / w as f64;
    ((max_width) & !1, ((h as f64 * scale).round() as u32) & !1)
}

/// `Physical size: 1080x1920` (an override line may follow and wins).
pub fn parse_wm_size(text: &str) -> Option<(u32, u32)> {
    text.lines()
        .filter_map(|l| l.split(':').nth(1))
        .filter_map(|v| v.trim().split_once('x'))
        .filter_map(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
        .last()
}

impl Mirror {
    /// Starts streaming. `send` gets every message; it returns false once the
    /// UI is gone, which stops the mirror.
    pub fn start(
        adb: std::path::PathBuf,
        serial: String,
        send: impl Fn(Message) -> bool + Send + 'static,
    ) -> Result<Mirror, String> {
        let size_text = run(&adb, &serial, &["shell", "wm", "size"])?;
        let device = parse_wm_size(&size_text).ok_or("Couldn't read the screen size.")?;
        let (w, h) = stream_size(device, 720);
        let stop = Arc::new(AtomicBool::new(false));
        let recorder: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        let (stop_t, recorder_t, adb_t, serial_t) = (stop.clone(), recorder.clone(), adb.clone(), serial.clone());
        thread::spawn(move || {
            let mut sent_setup = false;
            // screenrecord stops after 3 minutes; start it again until stopped.
            while !stop_t.load(Ordering::SeqCst) {
                let child = Command::new(&adb_t)
                    .args(["-s", &serial_t, "exec-out", "screenrecord", "--output-format=h264"])
                    .arg(format!("--size={w}x{h}"))
                    .args(["--bit-rate=6000000", "--time-limit=180", "-"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn();
                let mut child = match child {
                    Ok(c) => c,
                    Err(e) => {
                        send(Message::Ended(format!("Couldn't start the screen stream: {e}")));
                        return;
                    }
                };
                let mut out = child.stdout.take().expect("piped");
                *recorder_t.lock().unwrap() = Some(child);
                let mut splitter = FrameSplitter::default();
                let mut buf = vec![0u8; 64 << 10];
                let mut got_any = false;
                loop {
                    let n = match out.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    got_any = true;
                    for frame in splitter.push(&buf[..n]) {
                        if !sent_setup && frame.key {
                            if let Some(codec) = codec_string(&frame.data) {
                                sent_setup = true;
                                if !send(Message::Setup { codec, width: w, height: h, device_width: device.0, device_height: device.1 }) {
                                    stop_t.store(true, Ordering::SeqCst);
                                }
                            }
                        }
                        if sent_setup && !send(Message::Frame(frame)) {
                            stop_t.store(true, Ordering::SeqCst);
                        }
                    }
                    if stop_t.load(Ordering::SeqCst) {
                        break;
                    }
                }
                if let Some(mut c) = recorder_t.lock().unwrap().take() {
                    let _ = c.kill();
                }
                if !got_any && !stop_t.load(Ordering::SeqCst) {
                    send(Message::Ended("The device stopped sending its screen (it may have disconnected).".into()));
                    return;
                }
                thread::sleep(Duration::from_millis(200));
            }
        });
        Ok(Mirror { stop, recorder, shell: Mutex::new(None), adb, serial })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(mut c) = self.recorder.lock().unwrap().take() {
            let _ = c.kill();
        }
        if let Some((mut c, _)) = self.shell.lock().unwrap().take() {
            let _ = c.kill();
        }
    }

    /// Runs an `input` command through the persistent shell.
    pub fn input(&self, command: &str) -> Result<(), String> {
        let mut shell = self.shell.lock().unwrap();
        for attempt in 0..2 {
            if shell.is_none() {
                let mut child = Command::new(&self.adb)
                    .args(["-s", &self.serial, "shell"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Couldn't open a shell on the device: {e}"))?;
                let stdin = child.stdin.take().expect("piped");
                *shell = Some((child, stdin));
            }
            let (_, stdin) = shell.as_mut().unwrap();
            if writeln!(stdin, "{command}").and_then(|_| stdin.flush()).is_ok() {
                return Ok(());
            }
            // The shell died (device restarted?): open a new one once.
            *shell = None;
            if attempt == 1 {
                break;
            }
        }
        Err("Couldn't send input to the device.".into())
    }
}

impl Drop for Mirror {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(adb: &std::path::Path, serial: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(adb)
        .args(["-s", serial])
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A touch from the UI as an `input` command, in device pixels.
pub fn touch_command(x1: u32, y1: u32, x2: u32, y2: u32, millis: u32) -> String {
    // A swipe that doesn't move is a tap held for `millis`; some games ignore
    // the instant taps `input tap` makes.
    format!("input swipe {x1} {y1} {x2} {y2} {}", millis.clamp(40, 5000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_stream_into_frames_across_reads() {
        let sps = [0, 0, 0, 1, 0x67, 0x42, 0xC0, 0x29, 0xAA];
        let pps = [0, 0, 0, 1, 0x68, 0xCE];
        let idr = [0, 0, 0, 1, 0x65, 1, 2, 3];
        let p1 = [0, 0, 1, 0x41, 4, 5];
        let p2 = [0, 0, 0, 1, 0x41, 6];
        let stream: Vec<u8> = [&sps[..], &pps, &idr, &p1, &p2].concat();
        let mut s = FrameSplitter::default();
        let mut frames = Vec::new();
        // Fed in awkward pieces, like a pipe would.
        for chunk in stream.chunks(5) {
            frames.extend(s.push(chunk));
        }
        assert_eq!(frames.len(), 2, "the last frame waits for the next start code");
        assert!(frames[0].key);
        assert_eq!(frames[0].data, [&sps[..], &pps, &idr].concat());
        assert_eq!(frames[1], Frame { key: false, data: p1.to_vec() });
        assert_eq!(codec_string(&frames[0].data).as_deref(), Some("avc1.42C029"));
    }

    #[test]
    fn sizes_and_commands() {
        assert_eq!(parse_wm_size("Physical size: 1080x1920\n"), Some((1080, 1920)));
        assert_eq!(parse_wm_size("Physical size: 1080x1920\nOverride size: 720x1280\n"), Some((720, 1280)));
        assert_eq!(stream_size((1080, 1920), 720), (720, 1280));
        assert_eq!(stream_size((1921, 1081), 4000), (1920, 1080));
        assert_eq!(touch_command(10, 20, 10, 20, 5), "input swipe 10 20 10 20 40");
    }
}

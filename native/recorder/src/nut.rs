//! Just enough of the NUT container (<https://ffmpeg.org/nut.html>) to hand
//! `FFmpeg` raw frames with explicit microsecond timestamps and to read its
//! encoded packets back with theirs. A bare pipe carries no timestamps, and
//! linking libavformat here would clash with the `FFmpeg` symbols inside libwebrtc.

use std::io::{self, Read, Write};

const FILE_ID: &[u8] = b"nut/multimedia container\0";
const MAIN_STARTCODE: u64 = 0x4e4d_7a56_1f5f_04ad;
const STREAM_STARTCODE: u64 = 0x4e53_1140_5bf2_f9db;
const SYNCPOINT_STARTCODE: u64 = 0x4e4b_e4ad_eeca_4569;

const FLAG_KEY: u64 = 1;
const FLAG_CODED_PTS: u64 = 8;
const FLAG_STREAM_ID: u64 = 16;
const FLAG_SIZE_MSB: u64 = 32;
const FLAG_CHECKSUM: u64 = 64;
const FLAG_RESERVED: u64 = 128;
const FLAG_HEADER_INDEX: u64 = 1024;
const FLAG_MATCH_TIME: u64 = 2048;
const FLAG_CODED: u64 = 4096;
const FLAG_INVALID: u64 = 8192;

const MICROSECONDS_PER_SECOND: u64 = 1_000_000;
const MAXIMUM_DISTANCE: u64 = 65_536;
/// Timestamps below `1 << PTS_SHIFT` are read as low bits; ours are offset past it so
/// every coded value is a full timestamp.
const PTS_SHIFT: u64 = 14;
const MAXIMUM_PACKET_BYTES: u64 = 64 * 1024 * 1024;
/// A frame header that carries explicit flags (code 1 of the table below).
const FRAME_CODE: u8 = 1;
const MAXIMUM_HEADER_BYTES: u64 = 4096;
/// `FFmpeg`'s own frame-code table, which every NUT reader understands.
const FRAME_CODE_TABLE: [u8; 40] = [
    0xc0, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0xa0, 0x00, 0x02, 0x01, 0x01, 0x28, 0x01,
    0x00, 0x29, 0x00, 0x21, 0x01, 0x81, 0xff, 0x7f, 0x20, 0x02, 0x81, 0xff, 0x7f, 0x81, 0x79, 0xc0,
    0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn push_variable(bytes: &mut Vec<u8>, value: u64) {
    let groups = (64 - value.leading_zeros()).max(1).div_ceil(7);
    for group in (0..groups).rev() {
        let continuation = if group == 0 { 0 } else { 0x80 };
        bytes.push(continuation | ((value >> (7 * group)) & 0x7f) as u8);
    }
}

fn push_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    push_variable(bytes, value.len() as u64);
    bytes.extend_from_slice(value);
}

fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = 0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x04c1_1db7
            };
        }
    }
    crc
}

fn startcode_packet(startcode: u64, data: &[u8]) -> Vec<u8> {
    let mut packet = startcode.to_be_bytes().to_vec();
    push_variable(&mut packet, data.len() as u64 + 4);
    packet.extend_from_slice(data);
    packet.extend_from_slice(&checksum(data).to_be_bytes());
    packet
}

/// Writes one raw I420 video stream whose time base is one microsecond.
pub(super) struct NutWriter {
    width: u32,
    height: u32,
    started: bool,
}

impl NutWriter {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            started: false,
        }
    }

    fn headers(&self) -> Vec<u8> {
        let mut main = Vec::new();
        push_variable(&mut main, 3); // version
        push_variable(&mut main, 1); // streams
        push_variable(&mut main, MAXIMUM_DISTANCE);
        push_variable(&mut main, 1); // time bases
        push_variable(&mut main, 1);
        push_variable(&mut main, MICROSECONDS_PER_SECOND);
        main.extend_from_slice(&FRAME_CODE_TABLE);
        push_variable(&mut main, 0); // no elided headers beyond the empty one

        let mut stream = Vec::new();
        push_variable(&mut stream, 0); // stream id
        push_variable(&mut stream, 0); // video
        push_bytes(&mut stream, b"I420");
        push_variable(&mut stream, 0); // time base
        push_variable(&mut stream, PTS_SHIFT);
        push_variable(&mut stream, MICROSECONDS_PER_SECOND); // max pts distance
        push_variable(&mut stream, 0); // decode delay
        push_variable(&mut stream, 0); // flags
        push_bytes(&mut stream, b""); // codec specific data
        push_variable(&mut stream, u64::from(self.width));
        push_variable(&mut stream, u64::from(self.height));
        push_variable(&mut stream, 1); // sample aspect ratio
        push_variable(&mut stream, 1);
        push_variable(&mut stream, 0); // colorspace

        let mut bytes = FILE_ID.to_vec();
        bytes.extend(startcode_packet(MAIN_STARTCODE, &main));
        bytes.extend(startcode_packet(STREAM_STARTCODE, &stream));
        bytes
    }

    /// Write one frame stamped `timestamp_microseconds` after the first frame's origin.
    ///
    /// # Errors
    /// Returns the pipe's write error, or invalid input for a negative timestamp.
    pub fn write_frame(
        &mut self,
        output: &mut impl Write,
        timestamp_microseconds: i64,
        frame: &[u8],
    ) -> io::Result<()> {
        let timestamp =
            u64::try_from(timestamp_microseconds).map_err(|_| invalid("negative timestamp"))?;
        let mut bytes = if self.started {
            Vec::new()
        } else {
            self.started = true;
            self.headers()
        };
        // Every frame is a key frame, so each one is its own sync point.
        let mut syncpoint = Vec::new();
        push_variable(&mut syncpoint, timestamp);
        push_variable(&mut syncpoint, 0);
        bytes.extend(startcode_packet(SYNCPOINT_STARTCODE, &syncpoint));

        let mut header = vec![FRAME_CODE];
        let flags = FLAG_KEY | FLAG_CODED_PTS | FLAG_SIZE_MSB | FLAG_CHECKSUM;
        push_variable(&mut header, FLAG_CODED ^ flags);
        push_variable(&mut header, timestamp + (1 << PTS_SHIFT));
        push_variable(&mut header, frame.len() as u64);
        let header_checksum = checksum(&header);
        header.extend_from_slice(&header_checksum.to_be_bytes());
        bytes.extend(header);
        bytes.extend_from_slice(frame);
        output.write_all(&bytes)
    }
}

#[derive(Clone, Copy, Default)]
struct FrameCode {
    flags: u64,
    pts_delta: i64,
    multiplier: u64,
    size_lsb: u64,
    reserved: u64,
}

struct Slice<'a>(&'a [u8]);

impl Slice<'_> {
    fn byte(&mut self) -> io::Result<u8> {
        let (first, rest) = self
            .0
            .split_first()
            .ok_or_else(|| invalid("short header"))?;
        self.0 = rest;
        Ok(*first)
    }

    fn variable(&mut self) -> io::Result<u64> {
        let mut value = 0_u64;
        loop {
            let byte = self.byte()?;
            value = value
                .checked_mul(128)
                .ok_or_else(|| invalid("variable-length overflow"))?
                | u64::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
    }

    fn signed(&mut self) -> io::Result<i64> {
        let value = i64::try_from(self.variable()?).map_err(|_| invalid("signed overflow"))? + 1;
        Ok(if value & 1 == 1 {
            -(value >> 1)
        } else {
            value >> 1
        })
    }
}

/// An encoded packet in the stream's own time base units.
pub(super) struct NutPacket {
    pub timestamp: i64,
    pub data: Vec<u8>,
}

/// Reads packets from a NUT stream such as `FFmpeg`'s `-f nut` output.
pub(super) struct NutReader<R> {
    input: R,
    table: Vec<FrameCode>,
    pts_shift: u32,
    last_timestamp: i64,
    time_base: (u64, u64),
    time_base_count: u64,
    file_id_read: bool,
    started: bool,
}

impl<R: Read> NutReader<R> {
    pub fn new(input: R) -> Self {
        Self {
            input,
            table: vec![FrameCode::default(); 256],
            pts_shift: 0,
            last_timestamp: 0,
            time_base: (1, MICROSECONDS_PER_SECOND),
            time_base_count: 1,
            file_id_read: false,
            started: false,
        }
    }

    /// Seconds per timestamp unit as numerator and denominator.
    pub fn time_base(&self) -> (u64, u64) {
        self.time_base
    }

    fn read_byte(&mut self) -> io::Result<Option<u8>> {
        let mut byte = [0_u8];
        loop {
            return match self.input.read(&mut byte) {
                Ok(0) => Ok(None),
                Ok(_) => Ok(Some(byte[0])),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => Err(error),
            };
        }
    }

    fn require_byte(&mut self) -> io::Result<u8> {
        self.read_byte()?
            .ok_or_else(|| io::ErrorKind::UnexpectedEof.into())
    }

    fn read_variable(&mut self) -> io::Result<u64> {
        let mut value = 0_u64;
        loop {
            let byte = self.require_byte()?;
            value = value
                .checked_mul(128)
                .ok_or_else(|| invalid("variable-length overflow"))?
                | u64::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
    }

    fn skip(&mut self, count: u64) -> io::Result<()> {
        let copied = io::copy(&mut (&mut self.input).take(count), &mut io::sink())?;
        if copied == count {
            Ok(())
        } else {
            Err(io::ErrorKind::UnexpectedEof.into())
        }
    }

    fn read_exact_vec(&mut self, count: u64) -> io::Result<Vec<u8>> {
        if count > MAXIMUM_PACKET_BYTES {
            return Err(invalid("packet too large"));
        }
        let mut bytes = vec![0; usize::try_from(count).map_err(|_| invalid("packet too large"))?];
        self.input.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn read_startcode_packet(&mut self, first: u8) -> io::Result<()> {
        let mut startcode = [0_u8; 8];
        startcode[0] = first;
        self.input.read_exact(&mut startcode[1..])?;
        let startcode = u64::from_be_bytes(startcode);
        let length = self.read_variable()?;
        if length > MAXIMUM_HEADER_BYTES {
            self.skip(4)?; // header checksum
        }
        let body = self.read_exact_vec(length)?;
        let data = body
            .get(..body.len().saturating_sub(4))
            .ok_or_else(|| invalid("short packet"))?;
        match startcode {
            MAIN_STARTCODE => self.read_main_header(data)?,
            // A sync point restates the full time, which later frames' low bits are
            // coded relative to. Skipping it misreads every frame that is further
            // from the last one than the low bits can span.
            SYNCPOINT_STARTCODE => {
                let coded = Slice(data).variable()?;
                self.last_timestamp = i64::try_from(coded / self.time_base_count.max(1))
                    .map_err(|_| invalid("sync point timestamp"))?;
            }
            STREAM_STARTCODE => {
                let mut header = Slice(data);
                header.variable()?; // stream id
                header.variable()?; // class
                let fourcc = header.variable()?;
                for _ in 0..fourcc {
                    header.byte()?;
                }
                header.variable()?; // time base
                self.pts_shift = u32::try_from(header.variable()?)
                    .ok()
                    .filter(|shift| *shift <= 16)
                    .ok_or_else(|| invalid("timestamp shift"))?;
            }
            _ => {}
        }
        Ok(())
    }

    fn read_main_header(&mut self, data: &[u8]) -> io::Result<()> {
        let mut header = Slice(data);
        if header.variable()? > 3 {
            header.variable()?; // minor version
        }
        header.variable()?; // streams
        header.variable()?; // maximum distance
        let time_bases = header.variable()?;
        if time_bases == 0 {
            return Err(invalid("no time base"));
        }
        self.time_base_count = time_bases;
        for index in 0..time_bases {
            let numerator = header.variable()?;
            let denominator = header.variable()?;
            if index == 0 {
                self.time_base = (numerator, denominator);
            }
        }
        let mut index = 0_usize;
        let mut current = FrameCode::default();
        let mut stream_defaults = (0_i64, 1_u64);
        while index < 256 {
            current.flags = header.variable()?;
            let fields = header.variable()?;
            if fields > 0 {
                stream_defaults.0 = header.signed()?;
            }
            if fields > 1 {
                stream_defaults.1 = header.variable()?;
            }
            if fields > 2 {
                header.variable()?; // stream id
            }
            current.size_lsb = if fields > 3 { header.variable()? } else { 0 };
            current.reserved = if fields > 4 { header.variable()? } else { 0 };
            let count = if fields > 5 {
                header.variable()?
            } else {
                stream_defaults.1.saturating_sub(current.size_lsb)
            };
            if fields > 6 {
                header.signed()?;
            }
            if fields > 7 {
                header.variable()?;
            }
            for _ in 8..fields {
                header.variable()?;
            }
            current.pts_delta = stream_defaults.0;
            current.multiplier = stream_defaults.1;
            let mut offset = 0;
            while offset < count {
                if index >= 256 {
                    return Err(invalid("frame code table overflow"));
                }
                if index == usize::from(b'N') {
                    self.table[index] = FrameCode {
                        flags: FLAG_INVALID,
                        ..FrameCode::default()
                    };
                } else {
                    self.table[index] = FrameCode {
                        size_lsb: current.size_lsb + offset,
                        ..current
                    };
                    offset += 1;
                }
                index += 1;
            }
            if count == 0 {
                return Err(invalid("empty frame code run"));
            }
        }
        self.started = true;
        Ok(())
    }

    /// The next packet, or `None` at a clean end of stream.
    ///
    /// # Errors
    /// Returns an error for a truncated or malformed stream.
    pub fn next_packet(&mut self) -> io::Result<Option<NutPacket>> {
        if !self.file_id_read {
            let mut id = [0_u8; FILE_ID.len()];
            match self.input.read(&mut id[..1])? {
                0 => return Ok(None),
                _ => self.input.read_exact(&mut id[1..])?,
            }
            if id != FILE_ID {
                return Err(invalid("not a NUT stream"));
            }
            self.file_id_read = true;
        }
        loop {
            let Some(first) = self.read_byte()? else {
                return Ok(None);
            };
            if first == b'N' {
                self.read_startcode_packet(first)?;
                continue;
            }
            return self.read_frame(first).map(Some);
        }
    }

    fn read_frame(&mut self, frame_code: u8) -> io::Result<NutPacket> {
        if !self.started {
            return Err(invalid("frame before the main header"));
        }
        let code = self.table[usize::from(frame_code)];
        if code.flags & FLAG_INVALID != 0 {
            return Err(invalid("invalid frame code"));
        }
        let mut flags = code.flags;
        if flags & FLAG_CODED != 0 {
            flags ^= self.read_variable()?;
        }
        if flags & FLAG_STREAM_ID != 0 {
            self.read_variable()?;
        }
        let mut timestamp = self.last_timestamp + code.pts_delta;
        if flags & FLAG_CODED_PTS != 0 {
            let coded = i64::try_from(self.read_variable()?).map_err(|_| invalid("timestamp"))?;
            let shift = self.pts_shift;
            timestamp = if coded < (1_i64 << shift) {
                // Only the low bits were coded: pick the value nearest the last one.
                let mask = (1_i64 << shift) - 1;
                let base = self.last_timestamp - mask / 2;
                ((coded - base) & mask) + base
            } else {
                coded - (1_i64 << shift)
            };
        }
        let mut size = code.size_lsb;
        if flags & FLAG_SIZE_MSB != 0 {
            size = size
                .checked_add(
                    code.multiplier
                        .checked_mul(self.read_variable()?)
                        .ok_or_else(|| invalid("frame size"))?,
                )
                .ok_or_else(|| invalid("frame size"))?;
        }
        if flags & FLAG_MATCH_TIME != 0 {
            self.read_variable()?;
        }
        if flags & FLAG_HEADER_INDEX != 0 {
            self.read_variable()?;
        }
        let mut reserved = code.reserved;
        if flags & FLAG_RESERVED != 0 {
            reserved = self.read_variable()?;
        }
        for _ in 0..reserved {
            self.read_variable()?;
        }
        if flags & FLAG_CHECKSUM != 0 {
            self.skip(4)?;
        }
        self.last_timestamp = timestamp;
        Ok(NutPacket {
            timestamp,
            data: self.read_exact_vec(size)?,
        })
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "Test assertions fail immediately on unexpected outcomes"
)]
mod tests {
    use super::{NutReader, NutWriter};
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 480;
    const TIMESTAMPS: [i64; 5] = [0, 66_667, 175_111, 175_112, 5_000_000];

    fn frame(value: u8) -> Vec<u8> {
        // Larger than twice NUT's maximum distance, so every header needs its checksum.
        vec![value; (WIDTH * HEIGHT * 3 / 2) as usize]
    }

    fn written_stream() -> Vec<u8> {
        let mut writer = NutWriter::new(WIDTH, HEIGHT);
        let mut bytes = Vec::new();
        for (index, timestamp) in TIMESTAMPS.iter().enumerate() {
            let value = u8::try_from(index).unwrap() * 40;
            writer
                .write_frame(&mut bytes, *timestamp, &frame(value))
                .unwrap();
        }
        bytes
    }

    #[test]
    fn ffmpeg_reads_the_frames_and_timestamps_we_write() {
        let mut ffprobe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "frame=pts",
                "-of",
                "csv=p=0",
                "-f",
                "nut",
                "-i",
                "pipe:0",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        ffprobe
            .stdin
            .take()
            .unwrap()
            .write_all(&written_stream())
            .unwrap();
        let output = ffprobe.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let timestamps: Vec<i64> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| line.trim_end_matches(',').parse().unwrap())
            .collect();
        assert_eq!(timestamps, TIMESTAMPS);
    }

    #[test]
    fn we_read_the_packets_and_timestamps_ffmpeg_encodes() {
        let mut ffmpeg = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "nut",
                "-i",
                "pipe:0",
            ])
            .args(["-fps_mode", "vfr", "-enc_time_base", "1:1000000"])
            .args(["-c:v", "libx264", "-bf", "0", "-f", "nut", "pipe:1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut stdin = ffmpeg.stdin.take().unwrap();
        let input = written_stream();
        let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
        let mut reader = NutReader::new(ffmpeg.stdout.take().unwrap());
        let mut timestamps = Vec::new();
        while let Some(packet) = reader.next_packet().unwrap() {
            assert_ne!(packet.data.len(), 0);
            timestamps.push(packet.timestamp);
        }
        writer.join().unwrap();
        assert!(ffmpeg.wait().unwrap().success());
        assert_eq!(reader.time_base(), (1, 1_000_000));
        assert_eq!(timestamps, TIMESTAMPS);
    }

    #[test]
    fn frames_further_apart_than_the_low_bits_span_keep_their_timestamps() {
        const FRAME_INTERVAL_MICROSECONDS: i64 = 100_000;
        const FRAMES: i64 = 100;
        let mut ffmpeg = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "nut",
                "-i",
                "pipe:0",
            ])
            .args(["-fps_mode", "vfr", "-enc_time_base", "1:1000000"])
            .args([
                "-c:v", "libx264", "-bf", "0", "-g", "30", "-f", "nut", "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut stdin = ffmpeg.stdin.take().unwrap();
        let writer = std::thread::spawn(move || {
            let mut nut = NutWriter::new(WIDTH, HEIGHT);
            for index in 0..FRAMES {
                let value = u8::try_from(index).unwrap();
                nut.write_frame(
                    &mut stdin,
                    index * FRAME_INTERVAL_MICROSECONDS,
                    &frame(value),
                )
                .unwrap();
            }
        });
        let mut reader = NutReader::new(ffmpeg.stdout.take().unwrap());
        let mut timestamps = Vec::new();
        while let Some(packet) = reader.next_packet().unwrap() {
            timestamps.push(packet.timestamp);
        }
        writer.join().unwrap();
        assert!(ffmpeg.wait().unwrap().success());
        let expected: Vec<i64> = (0..FRAMES)
            .map(|index| index * FRAME_INTERVAL_MICROSECONDS)
            .collect();
        assert_eq!(timestamps, expected);
    }
}

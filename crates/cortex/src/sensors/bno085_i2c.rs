//! Minimal BNO085 SHTP-over-I2C reader.
//!
//! This is intentionally scoped to the reports Rudy needs today: rotation
//! vector, calibrated acceleration, and calibrated gyroscope.

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;

use crate::types::ImuSample;

const CHANNEL_EXE: u8 = 1;
const CHANNEL_CONTROL: u8 = 2;
const CHANNEL_INPUT_SENSOR_REPORTS: u8 = 3;

const REPORT_PRODUCT_ID_RESPONSE: u8 = 0xf8;
const REPORT_PRODUCT_ID_REQUEST: u8 = 0xf9;
const SET_FEATURE_COMMAND: u8 = 0xfd;
const GET_FEATURE_RESPONSE: u8 = 0xfc;

const REPORT_ACCELEROMETER: u8 = 0x01;
const REPORT_GYROSCOPE: u8 = 0x02;
const REPORT_ROTATION_VECTOR: u8 = 0x05;

const HEADER_LEN: usize = 4;
const MAX_PACKET_LEN: usize = 512;

#[derive(Debug, Clone, Copy)]
struct Header {
    packet_byte_count: usize,
    channel: u8,
    sequence: u8,
}

#[derive(Debug, Default, Clone)]
struct Readings {
    quaternion_xyzw: Option<[f32; 4]>,
    accel_m_s2: Option<[f32; 3]>,
    gyro_rad_s: Option<[f32; 3]>,
    rotation_accuracy: Option<u8>,
}

pub struct Bno085I2c {
    dev: LinuxI2CDevice,
    sequence: [u8; 6],
    readings: Readings,
}

impl Bno085I2c {
    pub fn open(bus: u8, address: u16) -> Result<Self> {
        let path = format!("/dev/i2c-{bus}");
        let dev = LinuxI2CDevice::new(&path, address)
            .with_context(|| format!("opening BNO085 on {path} address 0x{address:02x}"))?;
        Ok(Self {
            dev,
            sequence: [0; 6],
            readings: Readings::default(),
        })
    }

    pub fn initialize(&mut self, timeout: Duration, poll_interval_ms: u64) -> Result<()> {
        self.soft_reset();
        self.check_id(timeout)?;

        let interval_us = poll_interval_ms.max(5).saturating_mul(1_000);
        self.enable_report(REPORT_ROTATION_VECTOR, interval_us, timeout)?;
        self.enable_report(REPORT_ACCELEROMETER, interval_us, timeout)?;
        self.enable_report(REPORT_GYROSCOPE, interval_us, timeout)?;
        Ok(())
    }

    pub fn read_latest(&mut self) -> Result<Option<ImuSample>> {
        self.process_available_packets(10)?;
        let Some(quaternion_xyzw) = self.readings.quaternion_xyzw else {
            return Ok(None);
        };
        let Some(accel_m_s2) = self.readings.accel_m_s2 else {
            return Ok(None);
        };
        let Some(gyro_rad_s) = self.readings.gyro_rad_s else {
            return Ok(None);
        };
        let rotation_accuracy = self.readings.rotation_accuracy.unwrap_or(0);
        Ok(Some(ImuSample {
            quaternion_xyzw,
            accel_m_s2,
            gyro_rad_s,
            rotation_accuracy,
            rotation_accuracy_label: crate::sensors::rotation_accuracy_label(rotation_accuracy),
        }))
    }

    fn soft_reset(&mut self) {
        let _ = self.send_packet(CHANNEL_EXE, &[1]);
        std::thread::sleep(Duration::from_millis(500));
        let _ = self.send_packet(CHANNEL_EXE, &[1]);
        std::thread::sleep(Duration::from_millis(500));
        let _ = self.process_available_packets(3);
    }

    fn check_id(&mut self, timeout: Duration) -> Result<()> {
        self.send_packet(CHANNEL_CONTROL, &[REPORT_PRODUCT_ID_REQUEST, 0])?;
        let start = Instant::now();
        while start.elapsed() < timeout {
            if let Some(packet) = self.read_packet_if_ready()? {
                if packet.channel == CHANNEL_CONTROL
                    && packet.data.first().copied() == Some(REPORT_PRODUCT_ID_RESPONSE)
                {
                    return Ok(());
                }
                self.handle_packet(&packet)?;
            }
        }
        bail!("timed out waiting for BNO085 product id response")
    }

    fn enable_report(&mut self, report_id: u8, interval_us: u64, timeout: Duration) -> Result<()> {
        let mut payload = [0_u8; 17];
        payload[0] = SET_FEATURE_COMMAND;
        payload[1] = report_id;
        payload[5..9].copy_from_slice(&(interval_us as u32).to_le_bytes());
        self.send_packet(CHANNEL_CONTROL, &payload)?;

        let start = Instant::now();
        while start.elapsed() < timeout {
            self.process_available_packets(10)?;
            if self.report_has_data(report_id) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        bail!("timed out enabling BNO085 report 0x{report_id:02x}")
    }

    fn report_has_data(&self, report_id: u8) -> bool {
        match report_id {
            REPORT_ROTATION_VECTOR => self.readings.quaternion_xyzw.is_some(),
            REPORT_ACCELEROMETER => self.readings.accel_m_s2.is_some(),
            REPORT_GYROSCOPE => self.readings.gyro_rad_s.is_some(),
            _ => false,
        }
    }

    fn process_available_packets(&mut self, max_packets: usize) -> Result<()> {
        for _ in 0..max_packets {
            let Some(packet) = self.read_packet_if_ready()? else {
                return Ok(());
            };
            self.handle_packet(&packet)?;
        }
        Ok(())
    }

    fn read_packet_if_ready(&mut self) -> Result<Option<Packet>> {
        let header = self.read_header()?;
        if header.channel > 5 || header.packet_byte_count == 0 || header.packet_byte_count == 0x7fff
        {
            return Ok(None);
        }
        self.read_packet()
    }

    fn read_header(&mut self) -> Result<Header> {
        let mut buf = [0_u8; HEADER_LEN];
        self.dev.read(&mut buf).context("reading BNO085 header")?;
        Ok(parse_header(&buf))
    }

    fn read_packet(&mut self) -> Result<Option<Packet>> {
        let mut header_buf = [0_u8; HEADER_LEN];
        self.dev
            .read(&mut header_buf)
            .context("reading BNO085 packet header")?;
        let header = parse_header(&header_buf);
        if header.packet_byte_count == 0 {
            return Ok(None);
        }
        if header.packet_byte_count < HEADER_LEN || header.packet_byte_count > MAX_PACKET_LEN {
            bail!("invalid BNO085 packet length {}", header.packet_byte_count);
        }

        let mut buf = vec![0_u8; header.packet_byte_count];
        self.dev
            .read(&mut buf)
            .context("reading BNO085 packet body")?;
        let packet_header = parse_header(&buf[..HEADER_LEN]);
        if packet_header.channel < self.sequence.len() as u8 {
            self.sequence[packet_header.channel as usize] = packet_header.sequence;
        }
        Ok(Some(Packet {
            channel: packet_header.channel,
            data: buf[HEADER_LEN..packet_header.packet_byte_count].to_vec(),
        }))
    }

    fn send_packet(&mut self, channel: u8, data: &[u8]) -> Result<()> {
        let channel_idx = usize::from(channel);
        let seq = self
            .sequence
            .get_mut(channel_idx)
            .ok_or_else(|| anyhow!("invalid BNO085 channel {channel}"))?;
        let write_len = data.len() + HEADER_LEN;
        if write_len > u16::MAX as usize {
            bail!("BNO085 packet too large: {write_len} bytes");
        }

        let mut packet = Vec::with_capacity(write_len);
        packet.extend_from_slice(&(write_len as u16).to_le_bytes());
        packet.push(channel);
        packet.push(*seq);
        packet.extend_from_slice(data);
        self.dev.write(&packet).context("writing BNO085 packet")?;
        *seq = seq.wrapping_add(1);
        Ok(())
    }

    fn handle_packet(&mut self, packet: &Packet) -> Result<()> {
        if packet.channel != CHANNEL_CONTROL && packet.channel != CHANNEL_INPUT_SENSOR_REPORTS {
            return Ok(());
        }

        let mut offset = 0;
        while offset < packet.data.len() {
            let report_id = packet.data[offset];
            let Some(report_len) = report_len(report_id) else {
                return Ok(());
            };
            if offset + report_len > packet.data.len() {
                return Ok(());
            }
            self.handle_report(&packet.data[offset..offset + report_len])?;
            offset += report_len;
        }
        Ok(())
    }

    fn handle_report(&mut self, report: &[u8]) -> Result<()> {
        match report[0] {
            REPORT_ROTATION_VECTOR => {
                let values = parse_i16_values::<4>(report, 4, 1.0 / 16384.0)?;
                self.readings.quaternion_xyzw = Some(values);
                self.readings.rotation_accuracy = Some(report.get(2).copied().unwrap_or(0) & 0b11);
            }
            REPORT_ACCELEROMETER => {
                self.readings.accel_m_s2 = Some(parse_i16_values::<3>(report, 4, 1.0 / 256.0)?);
            }
            REPORT_GYROSCOPE => {
                self.readings.gyro_rad_s = Some(parse_i16_values::<3>(report, 4, 1.0 / 512.0)?);
            }
            GET_FEATURE_RESPONSE | REPORT_PRODUCT_ID_RESPONSE => {}
            _ => {}
        }
        Ok(())
    }
}

struct Packet {
    channel: u8,
    data: Vec<u8>,
}

fn parse_header(buf: &[u8]) -> Header {
    let raw = u16::from_le_bytes([buf[0], buf[1]]) & !0x8000;
    Header {
        packet_byte_count: usize::from(raw),
        channel: buf[2],
        sequence: buf[3],
    }
}

fn report_len(report_id: u8) -> Option<usize> {
    match report_id {
        REPORT_ACCELEROMETER | REPORT_GYROSCOPE => Some(10),
        REPORT_ROTATION_VECTOR => Some(14),
        GET_FEATURE_RESPONSE => Some(17),
        REPORT_PRODUCT_ID_RESPONSE => Some(16),
        0xfa | 0xfb => Some(5),
        _ => None,
    }
}

fn parse_i16_values<const N: usize>(report: &[u8], offset: usize, scalar: f32) -> Result<[f32; N]> {
    let mut out = [0.0_f32; N];
    for (idx, value) in out.iter_mut().enumerate() {
        let start = offset + idx * 2;
        let bytes = report
            .get(start..start + 2)
            .ok_or_else(|| anyhow!("short BNO085 report 0x{:02x}", report[0]))?;
        *value = f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) * scalar;
    }
    Ok(out)
}

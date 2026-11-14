// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

/// Result emitted by the background skin fetcher for one account ID.
pub struct SkinFetchResult {
    pub account_id: String,
    pub skin_url: Option<String>,
    pub head_argb: [u32; 64],
}

enum SkinJob {
    Fetch {
        account_id: String,
        uuid_hex: String,
        known_skin_url: Option<String>,
    },
}

/// Asynchronous player head avatar cache producing `8x8` ARGB pixel grids.
pub struct SkinAvatarCache {
    tx: Sender<SkinJob>,
    rx: Receiver<SkinFetchResult>,
    heads: HashMap<String, [u32; 64]>,
    in_flight: HashSet<String>,
}

impl SkinAvatarCache {
    /// Spawns a single daemon background worker thread for downloading and decoding skin heads.
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<SkinJob>();
        let (res_tx, res_rx) = mpsc::channel::<SkinFetchResult>();

        thread::Builder::new()
            .name("account-switcher-skins".into())
            .spawn(move || {
                let agent = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(8))
                    .timeout_read(Duration::from_secs(12))
                    .build();
                while let Ok(job) = job_rx.recv() {
                    let SkinJob::Fetch {
                        account_id,
                        uuid_hex,
                        known_skin_url,
                    } = job;
                    if let Some((url, grid)) =
                        fetch_head(&agent, &uuid_hex, known_skin_url.as_deref())
                    {
                        let _ = res_tx.send(SkinFetchResult {
                            account_id,
                            skin_url: Some(url),
                            head_argb: grid,
                        });
                    }
                }
            })
            .ok();

        Self {
            tx: job_tx,
            rx: res_rx,
            heads: HashMap::new(),
            in_flight: HashSet::new(),
        }
    }

    /// Seeds a cached grid directly from disk metadata so UI renders immediately.
    pub fn seed(&mut self, account_id: &str, head: [u32; 64]) {
        self.heads.insert(account_id.to_owned(), head);
    }

    /// Queues background skin retrieval once per account ID if not yet cached.
    pub fn ensure_requested(
        &mut self,
        account_id: &str,
        uuid_hex: &str,
        known_skin_url: Option<String>,
    ) {
        if self.heads.contains_key(account_id) || !self.in_flight.insert(account_id.to_owned()) {
            return;
        }
        let _ = self.tx.send(SkinJob::Fetch {
            account_id: account_id.to_owned(),
            uuid_hex: uuid_hex.to_owned(),
            known_skin_url,
        });
    }

    /// Returns ready `8x8` head pixels or a crisp deterministic fallback voxel head.
    pub fn get_or_fallback(&self, account_id: &str, username: &str, online: bool) -> [u32; 64] {
        self.heads
            .get(account_id)
            .copied()
            .unwrap_or_else(|| fallback_avatar(username, online))
    }

    /// Drains newly fetched heads since the last frame.
    pub fn poll_ready(&mut self) -> Vec<SkinFetchResult> {
        let mut ready = Vec::new();
        while let Ok(item) = self.rx.try_recv() {
            self.in_flight.remove(&item.account_id);
            self.heads.insert(item.account_id.clone(), item.head_argb);
            ready.push(item);
        }
        ready
    }
}

impl Default for SkinAvatarCache {
    fn default() -> Self {
        Self::new()
    }
}

fn fetch_head(
    agent: &ureq::Agent,
    uuid_hex: &str,
    known_skin_url: Option<&str>,
) -> Option<(String, [u32; 64])> {
    let url = match known_skin_url {
        Some(u) if is_allowed_texture_url(u) => u.to_owned(),
        _ => {
            let profile_url =
                format!("https://sessionserver.mojang.com/session/minecraft/profile/{uuid_hex}");
            let body = agent.get(&profile_url).call().ok()?.into_string().ok()?;
            extract_skin_url_from_sessionserver(&body)?
        }
    };
    if !is_allowed_texture_url(&url) {
        return None;
    }
    let resp = agent.get(&url).call().ok()?;
    let mut png_bytes = Vec::new();
    resp.into_reader().take(512 * 1024).read_to_end(&mut png_bytes).ok()?;
    let grid = composite_skin_head_png(&png_bytes)?;
    Some((url, grid))
}

fn is_allowed_texture_url(url: &str) -> bool {
    url.starts_with("https://textures.minecraft.net/texture/")
        || url.starts_with("http://textures.minecraft.net/texture/")
}

fn extract_skin_url_from_sessionserver(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let val_b64 = v
        .get("properties")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures"))?
        .get("value")?
        .as_str()?;
    let decoded = B64.decode(val_b64).ok()?;
    let tex_json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    tex_json
        .pointer("/textures/SKIN/url")
        .and_then(|u| u.as_str())
        .map(str::to_owned)
}

/// Decodes a Mojang PNG skin (`64x64` or `64x32`) and composites face `(8..16, 8..16)` with hat `(40..48, 8..16)`.
pub fn composite_skin_head_png(png_bytes: &[u8]) -> Option<[u32; 64]> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.width < 64 || info.height < 32 {
        return None;
    }
    let bpp = match info.color_type {
        png::ColorType::Rgba => 4usize,
        png::ColorType::Rgb => 3usize,
        _ => return None,
    };

    let sample = |x: usize, y: usize| -> [u8; 4] {
        let idx = (y * info.width as usize + x) * bpp;
        if bpp == 4 {
            [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]]
        } else {
            [buf[idx], buf[idx + 1], buf[idx + 2], 255]
        }
    };

    let mut out = [0u32; 64];
    for row in 0..8 {
        for col in 0..8 {
            let base = sample(8 + col, 8 + row);
            let hat = sample(40 + col, 8 + row);
            let [r, g, b] = alpha_blend_rgb(base, hat);
            out[row * 8 + col] = (0xFF << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }
    Some(out)
}

fn alpha_blend_rgb(base: [u8; 4], top: [u8; 4]) -> [u8; 3] {
    let a = top[3] as f32 / 255.0;
    let inv = 1.0 - a;
    let mix = |b: u8, t: u8| ((b as f32 * inv + t as f32 * a).round().clamp(0.0, 255.0)) as u8;
    [mix(base[0], top[0]), mix(base[1], top[1]), mix(base[2], top[2])]
}

/// Generates a stylized `8x8` voxel character avatar deterministically from username & mode.
pub fn fallback_avatar(username: &str, online: bool) -> [u32; 64] {
    let seed = username
        .bytes()
        .fold(0x811c9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x01000193));

    let palette: &[(u32, u32)] = if online {
        &[
            (0xFF6B4BFF, 0xFF3E249E),
            (0xFF4ADE80, 0xFF1F7A46),
            (0xFF38BDF8, 0xFF165B84),
            (0xFFA78BFA, 0xFF563EA8),
        ]
    } else {
        &[
            (0xFF94A3B8, 0xFF475569),
            (0xFFCBD5E1, 0xFF64748B),
            (0xFFF2C46B, 0xFF8E6822),
            (0xFFE2E8F0, 0xFF526072),
        ]
    };
    let (hair, shade) = palette[(seed as usize) % palette.len()];
    let skin = 0xFFE6C5AE;
    let skin_dark = 0xFFCFA78E;
    let eye_white = 0xFFF8FAFC;
    let eye_pupil = if online { 0xFF2E1E6B } else { 0xFF334155 };

    #[rustfmt::skip]
    let layout: [u8; 64] = [
        1, 1, 1, 1, 1, 1, 1, 1,
        1, 2, 2, 1, 1, 2, 2, 1,
        1, 0, 0, 0, 0, 0, 0, 1,
        0, 0, 0, 0, 0, 0, 0, 0,
        0, 4, 5, 0, 0, 5, 4, 0,
        0, 0, 0, 3, 3, 0, 0, 0,
        0, 0, 3, 0, 0, 3, 0, 0,
        0, 3, 3, 3, 3, 3, 3, 0,
    ];

    let mut out = [0u32; 64];
    for (i, &cell) in layout.iter().enumerate() {
        out[i] = match cell {
            1 => hair,
            2 => shade,
            3 => skin_dark,
            4 => eye_white,
            5 => eye_pupil,
            _ => skin,
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_sessionserver_skin_and_decodes_png_head() {
        let payload = B64.encode(br#"{"textures":{"SKIN":{"url":"https://textures.minecraft.net/texture/deadbeef"}}}"#);
        let doc = format!(r#"{{"properties":[{{"name":"textures","value":"{payload}"}}]}}"#);
        assert_eq!(
            extract_skin_url_from_sessionserver(&doc).as_deref(),
            Some("https://textures.minecraft.net/texture/deadbeef")
        );

        // Build minimal 64x64 RGBA PNG where face (8,8) is red and hat (40,8) is 50% green
        let mut pixels = vec![0u8; 64 * 64 * 4];
        let set_px = |buf: &mut [u8], x: usize, y: usize, rgba: [u8; 4]| {
            let idx = (y * 64 + x) * 4;
            buf[idx..idx + 4].copy_from_slice(&rgba);
        };
        set_px(&mut pixels, 8, 8, [200, 0, 0, 255]);
        set_px(&mut pixels, 40, 8, [0, 200, 0, 128]);
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&pixels).unwrap();
        }
        let head = composite_skin_head_png(&encoded).unwrap();
        let top_left = head[0];
        let r = (top_left >> 16) & 0xFF;
        let g = (top_left >> 8) & 0xFF;
        assert!(r >= 95 && r <= 105 && g >= 95 && g <= 105);
    }

    #[test]
    fn generates_deterministic_fallback_avatars() {
        let a1 = fallback_avatar("Cosmo", true);
        let a2 = fallback_avatar("Cosmo", true);
        let a3 = fallback_avatar("Cosmo", false);
        assert_eq!(a1, a2);
        assert_ne!(a1, a3);
    }
}

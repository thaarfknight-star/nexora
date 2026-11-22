//! Pure-Rust native CPU inference engine for **Nexora-MT v1** (`.nxmt`).
//!
//! Matches the exact architecture and binary format exported by
//! `nexora-mt/train_nexora_mt_colab.ipynb`:
//! - Binary header: `b"NXMT"` + `version: u32 LE (1)` + `header_len: u32 LE` + UTF-8 JSON header
//! - Tokenizer: HuggingFace ByteLevel BPE (`add_prefix_space = true`)
//! - Model: Pre-Norm RMSNorm (`eps = 1e-6`) + Multi-Head Attention + SwiGLU FFN
//!   Encoder-Decoder Transformer with weight-tied `tok_emb` / `lm_head`
//! - Quantization: Per-tensor symmetric INT8 (`i8`) for 2D weights, IEEE-754 `f16` for 1D norms
//! - Inference: Incremental KV-cache for decoder self-attention and precomputed encoder K/V
//!   for cross-attention, keeping RAM usage ~27 MB and CPU latency low.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const NXMT_MAGIC: &[u8; 4] = b"NXMT";
const NXMT_VERSION_MIN: u32 = 1;
const NXMT_VERSION_MAX: u32 = 2;
const BEAM_WIDTH: usize = 3;
const LENGTH_PENALTY_ALPHA: f32 = 0.6;
const RMS_EPS: f32 = 1e-6;
pub const DEFAULT_MAX_NEW_TOKENS: usize = 64;

#[derive(Debug, Clone, Deserialize)]
pub struct NexoraMtConfig {
    pub vocab_size: usize,
    pub d_model: usize,
    pub n_heads: usize,
    pub enc_layers: usize,
    pub dec_layers: usize,
    pub ffn_dim: usize,
    pub max_len: usize,
    pub pad_id: usize,
    pub bos_id: usize,
    pub eos_id: usize,
}

#[derive(Debug, Deserialize)]
struct TensorMeta {
    name: String,
    shape: Vec<usize>,
    dtype: String,
    scale: f32,
    offset: usize,
    nbytes: usize,
}

#[derive(Debug, Deserialize)]
struct NxmtHeader {
    cfg: NexoraMtConfig,
    tokenizer_json: String,
    tensors: Vec<TensorMeta>,
}

/// Decodes IEEE-754 binary16 (`f16`) little-endian bits into `f32`.
pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from((bits >> 15) & 1);
    let exp = u32::from((bits >> 10) & 0x1f);
    let mant = u32::from(bits & 0x03ff);

    let f32_bits = if exp == 0 {
        if mant == 0 {
            sign << 31
        } else {
            // Subnormal f16 -> normalized f32
            let mut m = mant;
            let mut e: i32 = -14;
            while (m & 0x0400) == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x03ff;
            let f_exp = u32::try_from(e + 127).unwrap_or(0);
            (sign << 31) | (f_exp << 23) | (m << 13)
        }
    } else if exp == 0x1f {
        (sign << 31) | (0xff << 23) | (mant << 13)
    } else {
        let f_exp = exp + (127 - 15);
        (sign << 31) | (f_exp << 23) | (mant << 13)
    };
    f32::from_bits(f32_bits)
}

/// GPT-2 / HuggingFace ByteLevel reversible byte <-> Unicode character mapping.
fn build_byte_level_tables() -> ([char; 256], HashMap<char, u8>) {
    let mut bs: Vec<u8> = Vec::with_capacity(256);
    bs.extend(b'!'..=b'~');
    bs.extend(0xA1u8..=0xACu8);
    bs.extend(0xAEu8..=0xFFu8);

    let mut cs: Vec<u32> = bs.iter().map(|&b| u32::from(b)).collect();
    let mut n = 0u32;
    for b in 0u8..=255u8 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + n);
            n += 1;
        }
    }

    let mut b2u = ['\0'; 256];
    let mut u2b = HashMap::with_capacity(256);
    for (b, c_u32) in bs.into_iter().zip(cs) {
        let ch = char::from_u32(c_u32).unwrap_or('?');
        b2u[usize::from(b)] = ch;
        u2b.insert(ch, b);
    }
    (b2u, u2b)
}

/// Minimal pure-Rust HuggingFace ByteLevel BPE tokenizer (`add_prefix_space = true`).
pub struct ByteLevelBpeTokenizer {
    vocab: HashMap<String, usize>,
    id_to_token: Vec<String>,
    merge_ranks: HashMap<(String, String), usize>,
    b2u: [char; 256],
    u2b: HashMap<char, u8>,
    unk_id: usize,
    special_ids: [usize; 4],
}

impl ByteLevelBpeTokenizer {
    pub fn from_json_str(raw: &str) -> Result<Self, String> {
        let val: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| format!("Invalid tokenizer JSON: {e}"))?;
        let model = val
            .get("model")
            .ok_or_else(|| "Tokenizer JSON missing 'model'".to_string())?;
        let vocab_obj = model
            .get("vocab")
            .and_then(|v| v.as_object())
            .ok_or_else(|| "Tokenizer JSON missing 'model.vocab'".to_string())?;

        let mut vocab = HashMap::with_capacity(vocab_obj.len());
        let mut max_id = 0usize;
        for (tok, id_val) in vocab_obj {
            if let Some(id_u64) = id_val.as_u64() {
                let id = id_u64 as usize;
                max_id = max_id.max(id);
                vocab.insert(tok.clone(), id);
            }
        }
        let mut id_to_token = vec![String::new(); max_id + 1];
        for (tok, &id) in &vocab {
            if id < id_to_token.len() {
                id_to_token[id] = tok.clone();
            }
        }

        let mut merge_ranks = HashMap::new();
        if let Some(merges) = model.get("merges").and_then(|m| m.as_array()) {
            for (rank, entry) in merges.iter().enumerate() {
                if let Some(s) = entry.as_str() {
                    if let Some((a, b)) = s.split_once(' ') {
                        merge_ranks.insert((a.to_string(), b.to_string()), rank);
                    }
                } else if let Some(pair) = entry.as_array() {
                    if pair.len() == 2 {
                        if let (Some(a), Some(b)) = (pair[0].as_str(), pair[1].as_str()) {
                            merge_ranks.insert((a.to_string(), b.to_string()), rank);
                        }
                    }
                }
            }
        }

        let unk_id = vocab.get("<unk>").copied().unwrap_or(3);
        let pad_id = vocab.get("<pad>").copied().unwrap_or(0);
        let bos_id = vocab.get("<bos>").copied().unwrap_or(1);
        let eos_id = vocab.get("<eos>").copied().unwrap_or(2);
        let (b2u, u2b) = build_byte_level_tables();

        Ok(Self {
            vocab,
            id_to_token,
            merge_ranks,
            b2u,
            u2b,
            unk_id,
            special_ids: [pad_id, bos_id, eos_id, unk_id],
        })
    }

    fn split_byte_level_words(text: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut current = String::new();
        for ch in text.chars() {
            if ch.is_whitespace() {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                current.push(ch);
            } else if ch.is_alphanumeric() {
                if !current.is_empty()
                    && !current
                        .chars()
                        .all(|c| c.is_whitespace() || c.is_alphanumeric())
                {
                    words.push(std::mem::take(&mut current));
                }
                current.push(ch);
            } else {
                if !current.is_empty() && !current.chars().all(char::is_whitespace) {
                    words.push(std::mem::take(&mut current));
                }
                current.push(ch);
                words.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            words.push(current);
        }
        words
    }

    fn bpe_word(&self, word: &str) -> Vec<String> {
        let mut symbols: Vec<String> = word
            .as_bytes()
            .iter()
            .map(|&b| self.b2u[usize::from(b)].to_string())
            .collect();
        if symbols.len() <= 1 {
            return symbols;
        }
        loop {
            let mut best_rank = usize::MAX;
            let mut best_idx = None;
            for i in 0..(symbols.len() - 1) {
                if let Some(&rank) = self
                    .merge_ranks
                    .get(&(symbols[i].clone(), symbols[i + 1].clone()))
                {
                    if rank < best_rank {
                        best_rank = rank;
                        best_idx = Some(i);
                    }
                }
            }
            let Some(idx) = best_idx else {
                break;
            };
            let merged = format!("{}{}", symbols[idx], symbols[idx + 1]);
            symbols.splice(idx..=idx + 1, std::iter::once(merged));
            if symbols.len() <= 1 {
                break;
            }
        }
        symbols
    }

    pub fn encode(&self, text: &str) -> Vec<usize> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        // ByteLevel pre-tokenizer uses `add_prefix_space = true`.
        let prefixed = format!(" {trimmed}");
        let mut ids = Vec::new();
        for piece in Self::split_byte_level_words(&prefixed) {
            for sym in self.bpe_word(&piece) {
                ids.push(self.vocab.get(&sym).copied().unwrap_or(self.unk_id));
            }
        }
        ids
    }

    pub fn decode(&self, ids: &[usize]) -> String {
        let mut bytes = Vec::with_capacity(ids.len() * 4);
        for &id in ids {
            if self.special_ids.contains(&id) {
                continue;
            }
            if let Some(tok) = self.id_to_token.get(id) {
                for ch in tok.chars() {
                    if let Some(&b) = self.u2b.get(&ch) {
                        bytes.push(b);
                    }
                }
            }
        }
        String::from_utf8_lossy(&bytes).trim().to_string()
    }
}

/// INT8 quantized 2D weight matrix `[rows, cols]` supporting both v1 (per-tensor)
/// and v2 (per-row / channel-wise) symmetric scales.
struct QuantMatrix {
    rows: usize,
    cols: usize,
    row_scales: Vec<f32>,
    data: Vec<i8>,
}

impl QuantMatrix {
    /// Computes `y = W * x` where `W` is `[rows, cols]` and `x` is `[cols]`.
    fn matvec(&self, x: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0f32; self.rows];
        let cols = self.cols;
        let (x_chunks, x_rem) = x.as_chunks::<4>();
        for (r, out_val) in out.iter_mut().enumerate() {
            let row = &self.data[r * cols..(r + 1) * cols];
            let (q_chunks, q_rem) = row.as_chunks::<4>();
            let mut a0 = 0.0f32;
            let mut a1 = 0.0f32;
            let mut a2 = 0.0f32;
            let mut a3 = 0.0f32;
            for (qc, xc) in q_chunks.iter().zip(x_chunks.iter()) {
                a0 += f32::from(qc[0]) * xc[0];
                a1 += f32::from(qc[1]) * xc[1];
                a2 += f32::from(qc[2]) * xc[2];
                a3 += f32::from(qc[3]) * xc[3];
            }
            let mut acc = (a0 + a1) + (a2 + a3);
            for (&q, &xv) in q_rem.iter().zip(x_rem.iter()) {
                acc += f32::from(q) * xv;
            }
            *out_val = acc * self.row_scales[r];
        }
        out
    }

    /// Reads row `idx` dequantized into `f32`, optionally multiplied by `mult`.
    fn row_f32(&self, idx: usize, mult: f32) -> Vec<f32> {
        let start = idx * self.cols;
        let s = self.row_scales[idx] * mult;
        self.data[start..start + self.cols]
            .iter()
            .map(|&q| f32::from(q) * s)
            .collect()
    }
}

struct AttentionWeights {
    q_proj: QuantMatrix,
    k_proj: QuantMatrix,
    v_proj: QuantMatrix,
    o_proj: QuantMatrix,
}

struct SwiGluWeights {
    w_gate: QuantMatrix,
    w_up: QuantMatrix,
    w_down: QuantMatrix,
}

struct EncoderLayerWeights {
    norm1: Vec<f32>,
    self_attn: AttentionWeights,
    norm2: Vec<f32>,
    ffn: SwiGluWeights,
}

struct DecoderLayerWeights {
    norm1: Vec<f32>,
    self_attn: AttentionWeights,
    norm2: Vec<f32>,
    cross_attn: AttentionWeights,
    norm3: Vec<f32>,
    ffn: SwiGluWeights,
}

/// Loaded native **Nexora-MT v1** translation engine.
pub struct NexoraMtEngine {
    cfg: NexoraMtConfig,
    tokenizer: ByteLevelBpeTokenizer,
    tok_emb: QuantMatrix,
    pos_emb: QuantMatrix,
    encoder: Vec<EncoderLayerWeights>,
    enc_norm: Vec<f32>,
    decoder: Vec<DecoderLayerWeights>,
    dec_norm: Vec<f32>,
}

fn rms_norm(x: &[f32], weight: &[f32]) -> Vec<f32> {
    let mean_sq = x.iter().map(|v| v * v).sum::<f32>() / (x.len().max(1) as f32);
    let inv = 1.0 / (mean_sq + RMS_EPS).sqrt();
    x.iter()
        .zip(weight.iter())
        .map(|(&v, &w)| v * inv * w)
        .collect()
}

fn swiglu_vec(x: &[f32], ffn: &SwiGluWeights) -> Vec<f32> {
    let gate = ffn.w_gate.matvec(x);
    let up = ffn.w_up.matvec(x);
    let hidden: Vec<f32> = gate
        .into_iter()
        .zip(up)
        .map(|(g, u)| {
            let silu = g / (1.0 + (-g).exp());
            silu * u
        })
        .collect();
    ffn.w_down.matvec(&hidden)
}

fn add_inplace(dst: &mut [f32], src: &[f32]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d += s;
    }
}

/// Computes single-query multi-head attention against key/value sequence slices.
fn mha_single_query(
    q: &[f32],
    keys: &[Vec<f32>],
    values: &[Vec<f32>],
    n_heads: usize,
    head_dim: usize,
) -> Vec<f32> {
    let d_model = n_heads * head_dim;
    let seq_len = keys.len();
    let inv_sqrt_hd = 1.0 / (head_dim as f32).sqrt();
    let mut concat = vec![0.0f32; d_model];
    let mut scores = vec![0.0f32; seq_len];

    for h in 0..n_heads {
        let h_off = h * head_dim;
        let q_h = &q[h_off..h_off + head_dim];

        let mut max_score = f32::NEG_INFINITY;
        for (t, k_t) in keys.iter().enumerate() {
            let k_h = &k_t[h_off..h_off + head_dim];
            let mut dot = 0.0f32;
            for i in 0..head_dim {
                dot += q_h[i] * k_h[i];
            }
            let s = dot * inv_sqrt_hd;
            scores[t] = s;
            if s > max_score {
                max_score = s;
            }
        }

        let mut denom = 0.0f32;
        for s in &mut scores {
            *s = (*s - max_score).exp();
            denom += *s;
        }
        let inv_denom = if denom > 0.0 { 1.0 / denom } else { 0.0 };

        let out_h = &mut concat[h_off..h_off + head_dim];
        for (t, v_t) in values.iter().enumerate() {
            let w = scores[t] * inv_denom;
            let v_h = &v_t[h_off..h_off + head_dim];
            for i in 0..head_dim {
                out_h[i] += w * v_h[i];
            }
        }
    }

    concat
}

/// Computes `(max_logit, sum_exp, top_k_tokens)` over the weight-tied `lm_head`.
fn lm_head_top_k(
    tok_emb: &QuantMatrix,
    final_h: &[f32],
    top_k: usize,
    n_workers: usize,
) -> (f32, f32, Vec<(usize, f32)>) {
    let cols = tok_emb.cols;
    let rows = tok_emb.rows;
    let data = &tok_emb.data;
    let row_scales = &tok_emb.row_scales;
    let (h_chunks, h_rem) = final_h.as_chunks::<4>();

    if rows >= 1024 && n_workers > 1 {
        let chunk_rows = rows.div_ceil(n_workers);
        let partials: Vec<(f32, f32, Vec<(usize, f32)>)> = std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(n_workers);
            for w in 0..n_workers {
                let start = w * chunk_rows;
                let end = (start + chunk_rows).min(rows);
                if start >= end {
                    continue;
                }
                handles.push(s.spawn(move || {
                    let mut logits = Vec::with_capacity(end - start);
                    let mut lmax = f32::NEG_INFINITY;
                    for v_id in start..end {
                        let row = &data[v_id * cols..(v_id + 1) * cols];
                        let (qc, qr) = row.as_chunks::<4>();
                        let mut a0 = 0.0f32;
                        let mut a1 = 0.0f32;
                        let mut a2 = 0.0f32;
                        let mut a3 = 0.0f32;
                        for (q4, h4) in qc.iter().zip(h_chunks.iter()) {
                            a0 += f32::from(q4[0]) * h4[0];
                            a1 += f32::from(q4[1]) * h4[1];
                            a2 += f32::from(q4[2]) * h4[2];
                            a3 += f32::from(q4[3]) * h4[3];
                        }
                        let mut dot = (a0 + a1) + (a2 + a3);
                        for (&q, &hv) in qr.iter().zip(h_rem.iter()) {
                            dot += f32::from(q) * hv;
                        }
                        let val = dot * row_scales[v_id];
                        if val > lmax {
                            lmax = val;
                        }
                        logits.push((v_id, val));
                    }
                    let mut sexp = 0.0f32;
                    for &(_, val) in &logits {
                        sexp += (val - lmax).exp();
                    }
                    let mut best: Vec<(usize, f32)> = Vec::with_capacity(top_k + 1);
                    for (id, val) in logits {
                        if best.len() < top_k
                            || val > best.last().map_or(f32::NEG_INFINITY, |x| x.1)
                        {
                            best.push((id, val));
                            best.sort_by(|a, b| {
                                b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                            });
                            best.truncate(top_k);
                        }
                    }
                    (lmax, sexp, best)
                }));
            }
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });

        let gmax = partials
            .iter()
            .map(|p| p.0)
            .fold(f32::NEG_INFINITY, f32::max);
        let gsum: f32 = partials.iter().map(|p| p.1 * (p.0 - gmax).exp()).sum();
        let mut merged: Vec<(usize, f32)> = partials.into_iter().flat_map(|p| p.2).collect();
        merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        merged.truncate(top_k);
        (gmax, gsum, merged)
    } else {
        let mut logits = Vec::with_capacity(rows);
        let mut lmax = f32::NEG_INFINITY;
        for v_id in 0..rows {
            let row = &data[v_id * cols..(v_id + 1) * cols];
            let mut dot = 0.0f32;
            for (&q, &hv) in row.iter().zip(final_h.iter()) {
                dot += f32::from(q) * hv;
            }
            let val = dot * row_scales[v_id];
            if val > lmax {
                lmax = val;
            }
            logits.push((v_id, val));
        }
        let sexp: f32 = logits.iter().map(|&(_, v)| (v - lmax).exp()).sum();
        logits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        logits.truncate(top_k);
        (lmax, sexp, logits)
    }
}

impl NexoraMtEngine {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| format!("Failed reading .nxmt model file: {e}"))?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 12 || &bytes[0..4] != NXMT_MAGIC {
            return Err("Invalid .nxmt file: bad magic header".to_string());
        }
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if !(NXMT_VERSION_MIN..=NXMT_VERSION_MAX).contains(&version) {
            return Err(format!("Unsupported .nxmt version: {version}"));
        }
        let header_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let payload_start = 12usize
            .checked_add(header_len)
            .ok_or_else(|| "Corrupt .nxmt header length".to_string())?;
        if payload_start > bytes.len() {
            return Err("Truncated .nxmt header".to_string());
        }

        let header: NxmtHeader = serde_json::from_slice(&bytes[12..payload_start])
            .map_err(|e| format!("Failed parsing .nxmt JSON header: {e}"))?;
        let payload = &bytes[payload_start..];
        let tokenizer = ByteLevelBpeTokenizer::from_json_str(&header.tokenizer_json)?;

        let mut meta_map: HashMap<&str, &TensorMeta> = HashMap::with_capacity(header.tensors.len());
        for t in &header.tensors {
            meta_map.insert(t.name.as_str(), t);
        }

        let load_mat = |name: &str| -> Result<QuantMatrix, String> {
            let m = meta_map
                .get(name)
                .ok_or_else(|| format!("Missing tensor '{name}' in .nxmt"))?;
            if m.shape.len() != 2 || (m.dtype != "i8" && m.dtype != "i8_row") {
                return Err(format!(
                    "Tensor '{name}' expected 2D i8/i8_row, got {} {:?}",
                    m.dtype, m.shape
                ));
            }
            let rows = m.shape[0];
            let cols = m.shape[1];
            let end = m
                .offset
                .checked_add(m.nbytes)
                .ok_or_else(|| format!("Overflow reading tensor '{name}'"))?;
            if end > payload.len() {
                return Err(format!("Truncated tensor '{name}'"));
            }
            let raw = &payload[m.offset..end];
            if m.dtype == "i8_row" {
                let scale_bytes = rows * 4;
                if m.nbytes != scale_bytes + rows * cols {
                    return Err(format!("Truncated i8_row tensor '{name}'"));
                }
                let mut row_scales = Vec::with_capacity(rows);
                for chunk in raw[..scale_bytes].as_chunks::<4>().0 {
                    row_scales.push(f32::from_le_bytes(*chunk));
                }
                let data: Vec<i8> = raw[scale_bytes..].iter().map(|&b| b as i8).collect();
                Ok(QuantMatrix {
                    rows,
                    cols,
                    row_scales,
                    data,
                })
            } else {
                if m.nbytes != rows * cols {
                    return Err(format!("Truncated i8 tensor '{name}'"));
                }
                let data: Vec<i8> = raw.iter().map(|&b| b as i8).collect();
                Ok(QuantMatrix {
                    rows,
                    cols,
                    row_scales: vec![m.scale; rows],
                    data,
                })
            }
        };

        let load_vec = |name: &str| -> Result<Vec<f32>, String> {
            let m = meta_map
                .get(name)
                .ok_or_else(|| format!("Missing tensor '{name}' in .nxmt"))?;
            if m.dtype != "f16" || m.shape.len() != 1 {
                return Err(format!(
                    "Tensor '{name}' expected 1D f16, got {} {:?}",
                    m.dtype, m.shape
                ));
            }
            let end = m
                .offset
                .checked_add(m.nbytes)
                .ok_or_else(|| format!("Overflow reading tensor '{name}'"))?;
            if end > payload.len() || m.nbytes != m.shape[0] * 2 {
                return Err(format!("Truncated tensor '{name}'"));
            }
            let raw = &payload[m.offset..end];
            let mut out = Vec::with_capacity(m.shape[0]);
            for chunk in raw.as_chunks::<2>().0 {
                out.push(f16_to_f32(u16::from_le_bytes([chunk[0], chunk[1]])));
            }
            Ok(out)
        };

        let load_attn = |prefix: &str| -> Result<AttentionWeights, String> {
            Ok(AttentionWeights {
                q_proj: load_mat(&format!("{prefix}.q_proj.weight"))?,
                k_proj: load_mat(&format!("{prefix}.k_proj.weight"))?,
                v_proj: load_mat(&format!("{prefix}.v_proj.weight"))?,
                o_proj: load_mat(&format!("{prefix}.o_proj.weight"))?,
            })
        };

        let load_ffn = |prefix: &str| -> Result<SwiGluWeights, String> {
            Ok(SwiGluWeights {
                w_gate: load_mat(&format!("{prefix}.w_gate.weight"))?,
                w_up: load_mat(&format!("{prefix}.w_up.weight"))?,
                w_down: load_mat(&format!("{prefix}.w_down.weight"))?,
            })
        };

        let tok_emb = load_mat("tok_emb.weight")?;
        let pos_emb = load_mat("pos_emb.weight")?;

        let mut encoder = Vec::with_capacity(header.cfg.enc_layers);
        for i in 0..header.cfg.enc_layers {
            encoder.push(EncoderLayerWeights {
                norm1: load_vec(&format!("encoder.{i}.norm1.weight"))?,
                self_attn: load_attn(&format!("encoder.{i}.self_attn"))?,
                norm2: load_vec(&format!("encoder.{i}.norm2.weight"))?,
                ffn: load_ffn(&format!("encoder.{i}.ffn"))?,
            });
        }
        let enc_norm = load_vec("enc_norm.weight")?;

        let mut decoder = Vec::with_capacity(header.cfg.dec_layers);
        for i in 0..header.cfg.dec_layers {
            decoder.push(DecoderLayerWeights {
                norm1: load_vec(&format!("decoder.{i}.norm1.weight"))?,
                self_attn: load_attn(&format!("decoder.{i}.self_attn"))?,
                norm2: load_vec(&format!("decoder.{i}.norm2.weight"))?,
                cross_attn: load_attn(&format!("decoder.{i}.cross_attn"))?,
                norm3: load_vec(&format!("decoder.{i}.norm3.weight"))?,
                ffn: load_ffn(&format!("decoder.{i}.ffn"))?,
            });
        }
        let dec_norm = load_vec("dec_norm.weight")?;

        Ok(Self {
            cfg: header.cfg,
            tokenizer,
            tok_emb,
            pos_emb,
            encoder,
            enc_norm,
            decoder,
            dec_norm,
        })
    }

    /// Translates an English sentence into Persian using greedy decoding with KV-cache.
    pub fn translate(&self, text: &str, max_new_tokens: usize) -> String {
        let clean = text.trim();
        if clean.is_empty() {
            return String::new();
        }
        let mut src_ids = Vec::with_capacity(self.cfg.max_len);
        src_ids.push(self.cfg.bos_id);
        for id in self
            .tokenizer
            .encode(clean)
            .into_iter()
            .take(self.cfg.max_len.saturating_sub(2))
        {
            src_ids.push(id.min(self.cfg.vocab_size.saturating_sub(1)));
        }
        src_ids.push(self.cfg.eos_id);

        let d_model = self.cfg.d_model;
        let n_heads = self.cfg.n_heads;
        let head_dim = d_model / n_heads;
        let emb_scale = (d_model as f32).sqrt();

        // 1. Encoder forward pass
        let mut hidden: Vec<Vec<f32>> = src_ids
            .iter()
            .enumerate()
            .map(|(pos, &tok)| {
                let mut v = self.tok_emb.row_f32(tok, emb_scale);
                let p = self.pos_emb.row_f32(pos, 1.0);
                add_inplace(&mut v, &p);
                v
            })
            .collect();

        for layer in &self.encoder {
            let normed: Vec<Vec<f32>> = hidden.iter().map(|h| rms_norm(h, &layer.norm1)).collect();
            let ks: Vec<Vec<f32>> = normed
                .iter()
                .map(|h| layer.self_attn.k_proj.matvec(h))
                .collect();
            let vs: Vec<Vec<f32>> = normed
                .iter()
                .map(|h| layer.self_attn.v_proj.matvec(h))
                .collect();
            for (t, h_norm) in normed.iter().enumerate() {
                let q = layer.self_attn.q_proj.matvec(h_norm);
                let ctx = mha_single_query(&q, &ks, &vs, n_heads, head_dim);
                let proj = layer.self_attn.o_proj.matvec(&ctx);
                add_inplace(&mut hidden[t], &proj);
                let h2 = rms_norm(&hidden[t], &layer.norm2);
                let ffn_out = swiglu_vec(&h2, &layer.ffn);
                add_inplace(&mut hidden[t], &ffn_out);
            }
        }
        let memory: Vec<Vec<f32>> = hidden.iter().map(|h| rms_norm(h, &self.enc_norm)).collect();

        // Precompute cross-attention K/V projections once per decoder layer
        type LayerKv = (Vec<Vec<f32>>, Vec<Vec<f32>>);
        let cross_kv: Vec<LayerKv> = self
            .decoder
            .iter()
            .map(|layer| {
                let ks = memory
                    .iter()
                    .map(|m| layer.cross_attn.k_proj.matvec(m))
                    .collect();
                let vs = memory
                    .iter()
                    .map(|m| layer.cross_attn.v_proj.matvec(m))
                    .collect();
                (ks, vs)
            })
            .collect();

        // Beam Search (beam = 3) with per-beam incremental KV-cache, length penalty,
        // and 3-gram repetition blocking for smoother, more natural Persian subtitles.
        #[derive(Clone)]
        struct Beam {
            tokens: Vec<usize>,
            log_prob: f32,
            finished: bool,
            self_k: Vec<Vec<Vec<f32>>>,
            self_v: Vec<Vec<Vec<f32>>>,
        }

        fn would_repeat_trigram(tokens: &[usize], next_tok: usize) -> bool {
            let n = tokens.len();
            if n >= 2 && tokens[n - 1] == next_tok && tokens[n - 2] == next_tok {
                return true;
            }
            if n < 3 {
                return false;
            }
            let a = tokens[n - 2];
            let b = tokens[n - 1];
            for i in 0..=(n - 3) {
                if tokens[i] == a && tokens[i + 1] == b && tokens[i + 2] == next_tok {
                    return true;
                }
            }
            false
        }

        fn norm_score(log_prob: f32, len: usize) -> f32 {
            let lp = ((5.0 + len as f32) / 6.0).powf(LENGTH_PENALTY_ALPHA);
            log_prob / lp
        }

        let adaptive_cap = src_ids.len().saturating_mul(2).saturating_add(8).min(48);
        let max_steps = max_new_tokens
            .min(adaptive_cap)
            .min(self.cfg.max_len.saturating_sub(1));

        let mut beams = vec![Beam {
            tokens: vec![self.cfg.bos_id],
            log_prob: 0.0,
            finished: false,
            self_k: vec![Vec::new(); self.decoder.len()],
            self_v: vec![Vec::new(); self.decoder.len()],
        }];

        let n_workers = std::thread::available_parallelism()
            .map(|n| n.get().clamp(1, 4))
            .unwrap_or(2);

        for step in 0..max_steps {
            if beams.iter().all(|b| b.finished) {
                break;
            }
            let mut candidates: Vec<Beam> = Vec::with_capacity(BEAM_WIDTH * BEAM_WIDTH);

            for beam in beams {
                if beam.finished {
                    candidates.push(beam);
                    continue;
                }
                let cur_tok = *beam.tokens.last().unwrap_or(&self.cfg.bos_id);
                let mut x = self.tok_emb.row_f32(cur_tok, emb_scale);
                let p = self.pos_emb.row_f32(step, 1.0);
                add_inplace(&mut x, &p);

                let mut next_k = beam.self_k;
                let mut next_v = beam.self_v;

                for (li, layer) in self.decoder.iter().enumerate() {
                    let h1 = rms_norm(&x, &layer.norm1);
                    let q = layer.self_attn.q_proj.matvec(&h1);
                    let k = layer.self_attn.k_proj.matvec(&h1);
                    let v = layer.self_attn.v_proj.matvec(&h1);
                    next_k[li].push(k);
                    next_v[li].push(v);
                    let ctx = mha_single_query(&q, &next_k[li], &next_v[li], n_heads, head_dim);
                    let proj = layer.self_attn.o_proj.matvec(&ctx);
                    add_inplace(&mut x, &proj);

                    let h2 = rms_norm(&x, &layer.norm2);
                    let cq = layer.cross_attn.q_proj.matvec(&h2);
                    let (ref cks, ref cvs) = cross_kv[li];
                    let cctx = mha_single_query(&cq, cks, cvs, n_heads, head_dim);
                    let cproj = layer.cross_attn.o_proj.matvec(&cctx);
                    add_inplace(&mut x, &cproj);

                    let h3 = rms_norm(&x, &layer.norm3);
                    let ffn_out = swiglu_vec(&h3, &layer.ffn);
                    add_inplace(&mut x, &ffn_out);
                }

                let final_h = rms_norm(&x, &self.dec_norm);
                let (max_logit, sum_exp, top_tokens) =
                    lm_head_top_k(&self.tok_emb, &final_h, BEAM_WIDTH + 2, n_workers);
                let log_denom = max_logit + sum_exp.max(1e-12).ln();
                let mut added = 0usize;
                for (tok_id, logit) in top_tokens {
                    if tok_id != self.cfg.eos_id && would_repeat_trigram(&beam.tokens, tok_id) {
                        continue;
                    }
                    let mut next_tokens = beam.tokens.clone();
                    let finished = tok_id == self.cfg.eos_id;
                    if !finished {
                        next_tokens.push(tok_id);
                    }
                    candidates.push(Beam {
                        tokens: next_tokens,
                        log_prob: beam.log_prob + (logit - log_denom),
                        finished,
                        self_k: next_k.clone(),
                        self_v: next_v.clone(),
                    });
                    added += 1;
                    if added >= BEAM_WIDTH {
                        break;
                    }
                }
            }

            candidates.sort_by(|a, b| {
                let sa = norm_score(a.log_prob, a.tokens.len().saturating_sub(1).max(1));
                let sb = norm_score(b.log_prob, b.tokens.len().saturating_sub(1).max(1));
                sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
            });
            candidates.truncate(BEAM_WIDTH);
            beams = candidates;
        }

        let best = beams.into_iter().next();
        let generated = best
            .map(|b| b.tokens.into_iter().skip(1).collect::<Vec<_>>())
            .unwrap_or_default();
        self.tokenizer.decode(&generated)
    }
}

struct CachedEngine {
    path: PathBuf,
    file_len: u64,
    engine: Arc<NexoraMtEngine>,
}

static RESIDENT_ENGINE: OnceLock<Mutex<Option<CachedEngine>>> = OnceLock::new();

/// Loads `model_path` into RAM once and reuses it across subtitle calls.
pub fn get_or_load_engine(model_path: &Path) -> Result<Arc<NexoraMtEngine>, String> {
    let meta = fs::metadata(model_path)
        .map_err(|_| "Local Nexora-MT v1 model file is not installed yet.".to_string())?;
    let file_len = meta.len();

    let slot = RESIDENT_ENGINE.get_or_init(|| Mutex::new(None));
    {
        let guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref cached) = *guard {
            if cached.path == model_path && cached.file_len == file_len {
                return Ok(Arc::clone(&cached.engine));
            }
        }
    }

    let engine = Arc::new(NexoraMtEngine::from_file(model_path)?);
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(CachedEngine {
        path: model_path.to_path_buf(),
        file_len,
        engine: Arc::clone(&engine),
    });
    Ok(engine)
}

/// Clears the resident cached engine so an updated `.nxmt` file reloads immediately.
pub fn invalidate_cached_engine() {
    if let Some(slot) = RESIDENT_ENGINE.get() {
        if let Ok(mut guard) = slot.lock() {
            *guard = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32_to_f16_one() -> [u8; 2] {
        // IEEE-754 f16 representation of 1.0 is 0x3C00
        0x3C00u16.to_le_bytes()
    }

    #[test]
    fn decodes_ieee754_f16_values() {
        assert!((f16_to_f32(0x3C00) - 1.0).abs() < 1e-6);
        assert!((f16_to_f32(0xBC00) + 1.0).abs() < 1e-6);
        assert!((f16_to_f32(0x4000) - 2.0).abs() < 1e-6);
        assert_eq!(f16_to_f32(0x0000), 0.0);
    }

    #[test]
    fn byte_level_bpe_roundtrips_english_and_persian_utf8() {
        let (b2u, _) = build_byte_level_tables();
        let mut vocab = serde_json::Map::new();
        vocab.insert("<pad>".into(), 0.into());
        vocab.insert("<bos>".into(), 1.into());
        vocab.insert("<eos>".into(), 2.into());
        vocab.insert("<unk>".into(), 3.into());
        for (idx, ch) in b2u.iter().enumerate() {
            vocab.insert(ch.to_string(), ((idx + 4) as u64).into());
        }
        let tok_json = serde_json::json!({
            "model": {
                "vocab": vocab,
                "merges": []
            }
        })
        .to_string();

        let tok = ByteLevelBpeTokenizer::from_json_str(&tok_json).expect("tokenizer loads");
        let en_ids = tok.encode("Hello Nexora!");
        assert_eq!(tok.decode(&en_ids), "Hello Nexora!");

        let fa_ids = tok.encode("سلام نکسورا");
        assert_eq!(tok.decode(&fa_ids), "سلام نکسورا");
    }

    #[test]
    fn loads_synthetic_nxmt_v1_and_runs_forward_pass() {
        let (b2u, _) = build_byte_level_tables();
        let mut vocab = serde_json::Map::new();
        vocab.insert("<pad>".into(), 0.into());
        vocab.insert("<bos>".into(), 1.into());
        vocab.insert("<eos>".into(), 2.into());
        vocab.insert("<unk>".into(), 3.into());
        for (idx, ch) in b2u.iter().enumerate() {
            vocab.insert(ch.to_string(), ((idx + 4) as u64).into());
        }
        let vocab_size = b2u.len() + 4;
        let d_model = 4usize;
        let ffn_dim = 8usize;
        let max_len = 16usize;

        let tok_json = serde_json::json!({
            "model": { "vocab": vocab, "merges": [] }
        })
        .to_string();

        let mut tensors_meta = Vec::new();
        let mut payload = Vec::new();

        fn push_mat(
            tensors_meta: &mut Vec<serde_json::Value>,
            payload: &mut Vec<u8>,
            name: &str,
            rows: usize,
            cols: usize,
            fill: i8,
        ) {
            let offset = payload.len();
            let nbytes = rows * cols;
            payload.extend(std::iter::repeat_n(fill as u8, nbytes));
            tensors_meta.push(serde_json::json!({
                "name": name,
                "shape": [rows, cols],
                "dtype": "i8",
                "scale": 0.01,
                "offset": offset,
                "nbytes": nbytes
            }));
        }
        fn push_vec(
            tensors_meta: &mut Vec<serde_json::Value>,
            payload: &mut Vec<u8>,
            name: &str,
            len: usize,
        ) {
            let offset = payload.len();
            let one = f32_to_f16_one();
            for _ in 0..len {
                payload.extend_from_slice(&one);
            }
            tensors_meta.push(serde_json::json!({
                "name": name,
                "shape": [len],
                "dtype": "f16",
                "scale": 1.0,
                "offset": offset,
                "nbytes": len * 2
            }));
        }

        // Make eos_id (2) row slightly positive so greedy decode terminates cleanly.
        let tok_offset = payload.len();
        let mut tok_bytes = vec![0u8; vocab_size * d_model];
        for c in 0..d_model {
            tok_bytes[2 * d_model + c] = 50u8;
        }
        payload.extend_from_slice(&tok_bytes);
        tensors_meta.push(serde_json::json!({
            "name": "tok_emb.weight",
            "shape": [vocab_size, d_model],
            "dtype": "i8",
            "scale": 0.01,
            "offset": tok_offset,
            "nbytes": vocab_size * d_model
        }));

        let m = &mut tensors_meta;
        let b = &mut payload;
        push_mat(m, b, "pos_emb.weight", max_len, d_model, 1);
        push_vec(m, b, "encoder.0.norm1.weight", d_model);
        for p_name in ["q_proj", "k_proj", "v_proj", "o_proj"] {
            let k = format!("encoder.0.self_attn.{p_name}.weight");
            push_mat(m, b, &k, d_model, d_model, 1);
        }
        push_vec(m, b, "encoder.0.norm2.weight", d_model);
        push_mat(m, b, "encoder.0.ffn.w_gate.weight", ffn_dim, d_model, 1);
        push_mat(m, b, "encoder.0.ffn.w_up.weight", ffn_dim, d_model, 1);
        push_mat(m, b, "encoder.0.ffn.w_down.weight", d_model, ffn_dim, 1);
        push_vec(m, b, "enc_norm.weight", d_model);

        push_vec(m, b, "decoder.0.norm1.weight", d_model);
        for p_name in ["q_proj", "k_proj", "v_proj", "o_proj"] {
            let k = format!("decoder.0.self_attn.{p_name}.weight");
            push_mat(m, b, &k, d_model, d_model, 1);
        }
        push_vec(m, b, "decoder.0.norm2.weight", d_model);
        for p_name in ["q_proj", "k_proj", "v_proj", "o_proj"] {
            let k = format!("decoder.0.cross_attn.{p_name}.weight");
            push_mat(m, b, &k, d_model, d_model, 1);
        }
        push_vec(m, b, "decoder.0.norm3.weight", d_model);
        push_mat(m, b, "decoder.0.ffn.w_gate.weight", ffn_dim, d_model, 1);
        push_mat(m, b, "decoder.0.ffn.w_up.weight", ffn_dim, d_model, 1);
        push_mat(m, b, "decoder.0.ffn.w_down.weight", d_model, ffn_dim, 1);
        push_vec(m, b, "dec_norm.weight", d_model);

        let header_json = serde_json::json!({
            "cfg": {
                "vocab_size": vocab_size,
                "d_model": d_model,
                "n_heads": 2,
                "enc_layers": 1,
                "dec_layers": 1,
                "ffn_dim": ffn_dim,
                "max_len": max_len,
                "pad_id": 0,
                "bos_id": 1,
                "eos_id": 2
            },
            "tokenizer_json": tok_json,
            "tensors": tensors_meta
        })
        .to_string();

        let mut file_bytes = Vec::new();
        file_bytes.extend_from_slice(NXMT_MAGIC);
        file_bytes.extend_from_slice(&NXMT_VERSION.to_le_bytes());
        file_bytes.extend_from_slice(&(header_json.len() as u32).to_le_bytes());
        file_bytes.extend_from_slice(header_json.as_bytes());
        file_bytes.extend_from_slice(&payload);

        let engine = NexoraMtEngine::from_bytes(&file_bytes).expect("synthetic .nxmt loads");
        let _out = engine.translate("Hello", 8);
    }

    #[test]
    fn loads_synthetic_nxmt_v2_per_row_int8_and_runs_beam_search() {
        let (b2u, _) = build_byte_level_tables();
        let mut vocab = serde_json::Map::new();
        vocab.insert("<pad>".into(), 0.into());
        vocab.insert("<bos>".into(), 1.into());
        vocab.insert("<eos>".into(), 2.into());
        vocab.insert("<unk>".into(), 3.into());
        for (idx, ch) in b2u.iter().enumerate() {
            vocab.insert(ch.to_string(), ((idx + 4) as u64).into());
        }
        let vocab_size = b2u.len() + 4;
        let d_model = 4usize;
        let ffn_dim = 8usize;
        let max_len = 16usize;
        let tok_json = serde_json::json!({
            "model": { "vocab": vocab, "merges": [] }
        })
        .to_string();

        let mut tensors_meta = Vec::new();
        let mut payload = Vec::new();
        fn push_row_mat(
            tensors_meta: &mut Vec<serde_json::Value>,
            payload: &mut Vec<u8>,
            name: &str,
            rows: usize,
            cols: usize,
            fill: i8,
        ) {
            let offset = payload.len();
            for _ in 0..rows {
                payload.extend_from_slice(&0.01f32.to_le_bytes());
            }
            payload.extend(std::iter::repeat_n(fill as u8, rows * cols));
            let nbytes = rows * 4 + rows * cols;
            tensors_meta.push(serde_json::json!({
                "name": name,
                "shape": [rows, cols],
                "dtype": "i8_row",
                "scale": 1.0,
                "offset": offset,
                "nbytes": nbytes
            }));
        }
        fn push_vec(
            tensors_meta: &mut Vec<serde_json::Value>,
            payload: &mut Vec<u8>,
            name: &str,
            len: usize,
        ) {
            let offset = payload.len();
            let one = f32_to_f16_one();
            for _ in 0..len {
                payload.extend_from_slice(&one);
            }
            tensors_meta.push(serde_json::json!({
                "name": name,
                "shape": [len],
                "dtype": "f16",
                "scale": 1.0,
                "offset": offset,
                "nbytes": len * 2
            }));
        }

        let m = &mut tensors_meta;
        let b = &mut payload;
        push_row_mat(m, b, "tok_emb.weight", vocab_size, d_model, 1);
        push_row_mat(m, b, "pos_emb.weight", max_len, d_model, 1);
        push_vec(m, b, "encoder.0.norm1.weight", d_model);
        for p_name in ["q_proj", "k_proj", "v_proj", "o_proj"] {
            let k = format!("encoder.0.self_attn.{p_name}.weight");
            push_row_mat(m, b, &k, d_model, d_model, 1);
        }
        push_vec(m, b, "encoder.0.norm2.weight", d_model);
        push_row_mat(m, b, "encoder.0.ffn.w_gate.weight", ffn_dim, d_model, 1);
        push_row_mat(m, b, "encoder.0.ffn.w_up.weight", ffn_dim, d_model, 1);
        push_row_mat(m, b, "encoder.0.ffn.w_down.weight", d_model, ffn_dim, 1);
        push_vec(m, b, "enc_norm.weight", d_model);

        push_vec(m, b, "decoder.0.norm1.weight", d_model);
        for p_name in ["q_proj", "k_proj", "v_proj", "o_proj"] {
            let k = format!("decoder.0.self_attn.{p_name}.weight");
            push_row_mat(m, b, &k, d_model, d_model, 1);
        }
        push_vec(m, b, "decoder.0.norm2.weight", d_model);
        for p_name in ["q_proj", "k_proj", "v_proj", "o_proj"] {
            let k = format!("decoder.0.cross_attn.{p_name}.weight");
            push_row_mat(m, b, &k, d_model, d_model, 1);
        }
        push_vec(m, b, "decoder.0.norm3.weight", d_model);
        push_row_mat(m, b, "decoder.0.ffn.w_gate.weight", ffn_dim, d_model, 1);
        push_row_mat(m, b, "decoder.0.ffn.w_up.weight", ffn_dim, d_model, 1);
        push_row_mat(m, b, "decoder.0.ffn.w_down.weight", d_model, ffn_dim, 1);
        push_vec(m, b, "dec_norm.weight", d_model);

        let header_json = serde_json::json!({
            "cfg": {
                "vocab_size": vocab_size,
                "d_model": d_model,
                "n_heads": 2,
                "enc_layers": 1,
                "dec_layers": 1,
                "ffn_dim": ffn_dim,
                "max_len": max_len,
                "pad_id": 0,
                "bos_id": 1,
                "eos_id": 2
            },
            "tokenizer_json": tok_json,
            "tensors": tensors_meta
        })
        .to_string();

        let mut file_bytes = Vec::new();
        file_bytes.extend_from_slice(NXMT_MAGIC);
        file_bytes.extend_from_slice(&2u32.to_le_bytes());
        file_bytes.extend_from_slice(&(header_json.len() as u32).to_le_bytes());
        file_bytes.extend_from_slice(header_json.as_bytes());
        file_bytes.extend_from_slice(&payload);

        let engine = NexoraMtEngine::from_bytes(&file_bytes).expect("synthetic .nxmt v2 loads");
        let _out = engine.translate("Hello Nexora", 6);
    }
}

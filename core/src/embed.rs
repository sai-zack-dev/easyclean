//! Semantic search: CLIP embeddings for photos and for the words you type.
//!
//! One model family, two encoders. The **vision** encoder turns a photo into a
//! 512-dimension vector; the **text** encoder turns a query into a vector in the
//! *same* space. Cosine similarity between them is the search score, which is
//! why "a crane at a harbour" can find a photo whose filename is `IMG_4821`.
//!
//! Three decisions worth knowing about:
//!
//! * **Embeddings come from the cached thumbnail, not the original.** CLIP
//!   resizes everything to 224x224 anyway, so re-decoding a 12MB HEIC would be
//!   pure waste — and it keeps the indexing pass off libheif entirely.
//! * **Vectors are stored int8 with a per-vector scale.** Four times smaller
//!   than float32 and about five times faster to scan, with no ranking change
//!   worth measuring.
//! * **There is no vector database.** An exact scan of 100K vectors takes
//!   ~15ms, which is faster than the 23ms the text encoder spends on the query
//!   itself. An ANN index would add a dependency to optimise the cheap half.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use image::DynamicImage;
use ort::session::Session;
use ort::value::Tensor;

use crate::error::{Error, Result};

/// Embedding width for CLIP ViT-B/32.
pub const DIM: usize = 512;
/// CLIP's fixed context length.
const CTX: usize = 77;
/// `<|endoftext|>`. CLIP pools at the *first* one, so padding with it is safe.
const EOT: i64 = 49407;
/// Square side the vision encoder expects.
const INPUT: u32 = 224;
/// Channel means/stds CLIP was trained with (from `preprocessor_config.json`).
const MEAN: [f32; 3] = [0.48145466, 0.4578275, 0.40821073];
const STD: [f32; 3] = [0.2686295, 0.2613026, 0.2757771];

/// Identifies the vector space. Stored on every row so that swapping models
/// invalidates the old vectors instead of comparing incomparable ones.
pub const MODEL_ID: &str = "clip-vit-b32-int8";

/// Where the weights are fetched from. Overridable so an air-gapped install can
/// point at a local mirror instead of Hugging Face.
const DEFAULT_BASE_URL: &str = "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main";

/// The three files that make up the model, and what each is for.
const FILES: [(&str, &str); 3] = [
    ("onnx/vision_model_quantized.onnx", "vision.onnx"),
    ("onnx/text_model_quantized.onnx", "text.onnx"),
    ("tokenizer.json", "tokenizer.json"),
];

/// Roughly what the download costs, for the UI to show before starting it.
pub const DOWNLOAD_BYTES: u64 = 149 * 1024 * 1024;

fn base_url() -> String {
    std::env::var("EASYCLEAN_CLIP_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}

/// Directory holding the downloaded weights.
pub fn model_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join(MODEL_ID)
}

/// True when every file the model needs is already on disk.
pub fn is_downloaded(data_dir: &Path) -> bool {
    let dir = model_dir(data_dir);
    FILES.iter().all(|(_, local)| dir.join(local).is_file())
}

/// Fetch the weights into `<data_dir>/models/<model>/`, reporting bytes so far.
///
/// Each file lands on a temporary path and is renamed only once complete, so an
/// interrupted download can never leave a half-written model that looks valid
/// to [`is_downloaded`].
pub fn download(data_dir: &Path, mut progress: impl FnMut(u64, u64)) -> Result<()> {
    let dir = model_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let base = base_url();
    let mut done: u64 = 0;

    for (remote, local) in FILES {
        let dest = dir.join(local);
        if dest.is_file() {
            done += std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            progress(done, DOWNLOAD_BYTES);
            continue;
        }
        let url = format!("{base}/{remote}");
        let resp = ureq::get(&url)
            .call()
            .map_err(|e| Error::Other(format!("downloading {remote}: {e}")))?;

        let tmp = dir.join(format!("{local}.part"));
        let mut out = std::fs::File::create(&tmp)?;
        let mut reader = resp.into_reader();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = std::io::Read::read(&mut reader, &mut buf)?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut out, &buf[..n])?;
            done += n as u64;
            progress(done, DOWNLOAD_BYTES.max(done));
        }
        drop(out);
        std::fs::rename(&tmp, &dest)?;
    }
    Ok(())
}

/// Embed a file straight from a path, decoding it first.
///
/// Kept here rather than in the server so the decode and the encoder stay in
/// one crate — the server never needs an image decoder of its own.
pub fn embed_image_at(clip: &Clip, path: &std::path::Path) -> Result<Vec<f32>> {
    let img = image::open(path)?;
    clip.embed_image(&img)
}

/// A loaded CLIP pair, ready to embed.
///
/// `Session` needs `&mut` to run, so each is behind its own mutex: embedding a
/// photo and encoding a query can then proceed independently, and the indexing
/// pass never blocks a search on the text encoder.
pub struct Clip {
    vision: Mutex<Session>,
    text: Mutex<Session>,
    tokenizer: tokenizers::Tokenizer,
}

impl Clip {
    /// Load the model from `<data_dir>/models/…`. Costs about a second.
    pub fn load(data_dir: &Path) -> Result<Self> {
        let dir = model_dir(data_dir);
        let open = |name: &str| -> Result<Session> {
            Session::builder()
                .and_then(|mut b| b.commit_from_file(dir.join(name)))
                .map_err(|e| Error::Other(format!("loading {name}: {e}")))
        };
        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| Error::Other(format!("loading tokenizer: {e}")))?;
        Ok(Self {
            vision: Mutex::new(open("vision.onnx")?),
            text: Mutex::new(open("text.onnx")?),
            tokenizer,
        })
    }

    /// Embed one already-decoded image. The result is L2-normalised, so a dot
    /// product against another such vector *is* the cosine similarity.
    pub fn embed_image(&self, img: &DynamicImage) -> Result<Vec<f32>> {
        let pixels = preprocess(img);
        let mut session = self
            .vision
            .lock()
            .map_err(|_| Error::Other("vision session poisoned".into()))?;
        let input = Tensor::from_array(([1usize, 3, INPUT as usize, INPUT as usize], pixels))
            .map_err(|e| Error::Other(format!("vision input: {e}")))?;
        let out = session
            .run(ort::inputs!["pixel_values" => input])
            .map_err(|e| Error::Other(format!("vision run: {e}")))?;
        let (_, raw) = out["image_embeds"]
            .try_extract_tensor::<f32>()
            .map_err(|e| Error::Other(format!("vision output: {e}")))?;
        let mut v = raw.to_vec();
        normalize(&mut v);
        Ok(v)
    }

    /// Embed a search phrase into the same space as [`Self::embed_image`].
    pub fn embed_text(&self, query: &str) -> Result<Vec<f32>> {
        let enc = self
            .tokenizer
            .encode(query, true)
            .map_err(|e| Error::Other(format!("tokenizing: {e}")))?;
        let mut ids: Vec<i64> = enc.get_ids().iter().map(|&i| i as i64).collect();
        ids.truncate(CTX);
        ids.resize(CTX, EOT);

        let mut session = self
            .text
            .lock()
            .map_err(|_| Error::Other("text session poisoned".into()))?;
        let input = Tensor::from_array(([1usize, CTX], ids))
            .map_err(|e| Error::Other(format!("text input: {e}")))?;
        let out = session
            .run(ort::inputs!["input_ids" => input])
            .map_err(|e| Error::Other(format!("text run: {e}")))?;
        let (_, raw) = out["text_embeds"]
            .try_extract_tensor::<f32>()
            .map_err(|e| Error::Other(format!("text output: {e}")))?;
        let mut v = raw.to_vec();
        normalize(&mut v);
        Ok(v)
    }
}

/// CLIP preprocessing: shortest side to 224 (bicubic), centre-crop, normalise
/// per channel, laid out NCHW.
fn preprocess(img: &DynamicImage) -> Vec<f32> {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let scale = INPUT as f32 / w.min(h).max(1.0);
    let nw = ((w * scale).round() as u32).max(INPUT);
    let nh = ((h * scale).round() as u32).max(INPUT);
    let resized = img.resize_exact(nw, nh, image::imageops::FilterType::CatmullRom);
    let cropped = resized
        .crop_imm((nw - INPUT) / 2, (nh - INPUT) / 2, INPUT, INPUT)
        .to_rgb8();

    let plane = (INPUT * INPUT) as usize;
    let mut out = vec![0f32; 3 * plane];
    for (x, y, px) in cropped.enumerate_pixels() {
        let at = y as usize * INPUT as usize + x as usize;
        for c in 0..3 {
            out[c * plane + at] = (px.0[c] as f32 / 255.0 - MEAN[c]) / STD[c];
        }
    }
    out
}

/// Scale to unit length, so later comparisons are a plain dot product.
fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-12 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

/// Pack a normalised vector into int8 plus the scale needed to read it back.
pub fn quantize(v: &[f32]) -> (Vec<u8>, f32) {
    let max = v.iter().fold(0f32, |m, x| m.max(x.abs())).max(1e-6);
    let step = max / 127.0;
    let bytes = v.iter().map(|x| ((x / step).round() as i8) as u8).collect();
    (bytes, step)
}

/// Similarity between a float query and a stored int8 row.
///
/// The stored vector's scale is a positive constant across the whole row, so it
/// rescales every candidate's score identically — it cannot reorder results,
/// but it is applied anyway so the number returned is a real cosine value the
/// UI can reason about rather than an arbitrary magnitude.
pub fn similarity(query: &[f32], stored: &[u8], scale: f32) -> f32 {
    if stored.len() != query.len() {
        return f32::MIN;
    }
    let dot: f32 = query
        .iter()
        .zip(stored)
        .map(|(q, s)| q * (*s as i8) as f32)
        .sum();
    dot * scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn preprocess_always_yields_one_clip_input() {
        // Whatever goes in — wide panorama, tall portrait, tiny thumbnail — the
        // encoder only ever accepts 3x224x224.
        for (w, h) in [(640, 480), (100, 4000), (4000, 100), (50, 50), (224, 224)] {
            let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([120, 30, 200])));
            let px = preprocess(&img);
            assert_eq!(px.len(), 3 * 224 * 224, "{w}x{h}");
            assert!(px.iter().all(|v| v.is_finite()), "{w}x{h} produced NaN");
        }
    }

    #[test]
    fn normalized_vectors_are_unit_length() {
        let mut v = vec![3.0f32, 4.0];
        normalize(&mut v);
        assert!((v.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-6);

        // A zero vector must survive rather than turn into NaN.
        let mut z = vec![0.0f32; 4];
        normalize(&mut z);
        assert!(z.iter().all(|x| x.is_finite()));
    }

    /// Quantising must not reorder results — that is the whole bet behind
    /// storing int8 instead of float32.
    #[test]
    fn quantized_similarity_preserves_ranking() {
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((seed >> 33) as f32 / (1u64 << 31) as f32) - 1.0
        };
        let mut make = || {
            let mut v: Vec<f32> = (0..DIM).map(|_| rnd()).collect();
            normalize(&mut v);
            v
        };

        let query = make();
        let corpus: Vec<Vec<f32>> = (0..64).map(|_| make()).collect();

        let mut exact: Vec<(usize, f32)> = corpus
            .iter()
            .enumerate()
            .map(|(i, c)| (i, query.iter().zip(c).map(|(a, b)| a * b).sum::<f32>()))
            .collect();
        exact.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let mut approx: Vec<(usize, f32)> = corpus
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (bytes, scale) = quantize(c);
                (i, similarity(&query, &bytes, scale))
            })
            .collect();
        approx.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        assert_eq!(
            exact[0].0, approx[0].0,
            "int8 must pick the same nearest neighbour as float32"
        );
        let exact_top: Vec<usize> = exact[..10].iter().map(|x| x.0).collect();
        let approx_top: Vec<usize> = approx[..10].iter().map(|x| x.0).collect();
        let overlap = approx_top.iter().filter(|i| exact_top.contains(i)).count();
        assert!(overlap >= 9, "top-10 overlap was only {overlap}/10");
    }

    #[test]
    fn mismatched_widths_never_rank_highly() {
        let q = vec![0.5f32; DIM];
        assert_eq!(similarity(&q, &[1u8, 2, 3], 1.0), f32::MIN);
    }
}

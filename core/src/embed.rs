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

/// Repository the weights come from.
const DEFAULT_REPO_URL: &str = "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve";

/// The exact upstream commit the weights are pinned to.
///
/// A commit SHA rather than `main`, because `main` is whatever the repository
/// owner last pushed. An `.onnx` file is not data — it is a compute graph that
/// [`Clip::load`] hands straight to ONNX Runtime in this process — so fetching
/// "the current version" means running whatever code a third party publishes
/// next, on a machine that holds someone's entire photo library. Pinned, the
/// only thing that can change under us is nothing.
const MODEL_REVISION: &str = "d15189d7028b43f1d3e65039190477f6af591c2a";

/// Name of the marker written once every file has been verified.
const VERIFIED_MARKER: &str = ".verified";

/// The three files that make up the model: remote path, local name, the
/// SHA-256 it must have, and its size.
///
/// The digests are the actual control. The revision pin says *which* bytes we
/// want; these prove we got them — over a plain-HTTP mirror, through a redirect,
/// past a proxy, or from an upstream that was tampered with after we pinned it.
const FILES: [(&str, &str, &str, u64); 3] = [
    (
        "onnx/vision_model_quantized.onnx",
        "vision.onnx",
        "583fd1110a514667812fee7d684952aaf82a99b959760c8d7dca7e0ab9839299",
        89_117_001,
    ),
    (
        "onnx/text_model_quantized.onnx",
        "text.onnx",
        "73baab855d406190da9faa498cfedf65f15cf309f4cc7385b7b032e6d08e5c3a",
        64_504_507,
    ),
    (
        "tokenizer.json",
        "tokenizer.json",
        "f7f3b7af117d467b58374797691a6438d3e6b9e9cef800dfd5dced7f697a90cd",
        2_224_119,
    ),
];

/// What the download costs, for the UI to show before starting it. Exact, since
/// the revision is pinned.
pub const DOWNLOAD_BYTES: u64 = 155_845_627;

/// Where the weights are fetched from. Overridable so an air-gapped install can
/// point at a local mirror — the digests above still apply, so a mirror has to
/// serve the same bytes, not merely something that loads.
fn base_url() -> String {
    std::env::var("EASYCLEAN_CLIP_BASE_URL")
        .unwrap_or_else(|_| format!("{DEFAULT_REPO_URL}/{MODEL_REVISION}"))
}

/// Directory holding the downloaded weights.
pub fn model_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join(MODEL_ID)
}

/// True when every file the model needs is on disk *and* was verified against
/// the pinned digests at this revision.
///
/// The marker carries the revision, so weights left by a build that fetched
/// them before they were pinned — or from a different revision — read as not
/// downloaded and go through [`download`] again, which checks them rather than
/// trusting whatever is sitting in the directory.
pub fn is_downloaded(data_dir: &Path) -> bool {
    let dir = model_dir(data_dir);
    FILES
        .iter()
        .all(|(_, local, _, _)| dir.join(local).is_file())
        && marker_is_current(&dir)
}

fn marker_is_current(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(VERIFIED_MARKER))
        .map(|s| s.trim() == MODEL_REVISION)
        .unwrap_or(false)
}

/// SHA-256 of a file already on disk, hex-encoded.
fn sha256_file(path: &Path) -> Result<String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_of(&hasher.finalize()))
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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

    for (remote, local, want_sha, size) in FILES {
        let dest = dir.join(local);

        // Already here? Only reuse it if it hashes to what we expect. That is
        // what lets a pre-pin download be salvaged when its bytes happen to be
        // right, and re-fetched when they are not — without ever trusting the
        // file just because it exists.
        if dest.is_file() {
            match sha256_file(&dest) {
                Ok(have) if have == want_sha => {
                    done += size;
                    progress(done, DOWNLOAD_BYTES);
                    continue;
                }
                _ => {
                    tracing::warn!("{local} is not the pinned build — fetching it again");
                    let _ = std::fs::remove_file(&dest);
                }
            }
        }

        let url = format!("{base}/{remote}");
        let resp = ureq::get(&url)
            .call()
            .map_err(|e| Error::Other(format!("downloading {remote}: {e}")))?;

        let tmp = dir.join(format!("{local}.part"));
        let mut out = std::fs::File::create(&tmp)?;
        let mut reader = resp.into_reader();
        let mut buf = vec![0u8; 1 << 16];
        // Hashed as it streams, so verifying costs no second pass over 150MB.
        let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
        loop {
            let n = std::io::Read::read(&mut reader, &mut buf)?;
            if n == 0 {
                break;
            }
            sha2::Digest::update(&mut hasher, &buf[..n]);
            std::io::Write::write_all(&mut out, &buf[..n])?;
            done += n as u64;
            progress(done, DOWNLOAD_BYTES.max(done));
        }
        drop(out);

        // Checked before the rename, so a file that fails never reaches the
        // name `Clip::load` opens — there is no window in which a bad graph is
        // loadable, and no half-verified state to clean up later.
        let have = hex_of(&sha2::Digest::finalize(hasher));
        if have != want_sha {
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::Other(format!(
                "{remote} does not match the pinned build: expected sha256 {want_sha}, got {have}. \
                 Refusing to load it. Check EASYCLEAN_CLIP_BASE_URL, or the network between here \
                 and it."
            )));
        }
        std::fs::rename(&tmp, &dest)?;
    }

    // Written last: its presence is the claim that everything beside it was
    // checked at this revision.
    std::fs::write(dir.join(VERIFIED_MARKER), MODEL_REVISION)?;
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
    fn sha256_matches_a_known_value() {
        let dir = std::env::temp_dir().join(format!("ec-sha-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x");
        std::fs::write(&f, b"abc").unwrap();
        assert_eq!(
            sha256_file(&f).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Files on disk are not evidence that they are the right files.
    ///
    /// Before the weights were pinned, `is_downloaded` asked only whether the
    /// three names existed — so anything that had once been fetched, from any
    /// revision and over any connection, was loaded into ONNX Runtime without
    /// another thought. The marker is what makes "downloaded" mean "checked".
    #[test]
    fn present_but_unverified_weights_do_not_count_as_downloaded() {
        let data = std::env::temp_dir().join(format!("ec-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let dir = model_dir(&data);
        std::fs::create_dir_all(&dir).unwrap();

        for (_, local, _, _) in FILES {
            std::fs::write(dir.join(local), b"not really the model").unwrap();
        }
        assert!(
            !is_downloaded(&data),
            "unverified files were treated as a usable model"
        );

        // A marker from some other revision is not this revision's evidence.
        std::fs::write(
            dir.join(VERIFIED_MARKER),
            "0000000000000000000000000000000000000000",
        )
        .unwrap();
        assert!(!is_downloaded(&data), "a stale marker was accepted");

        std::fs::write(dir.join(VERIFIED_MARKER), MODEL_REVISION).unwrap();
        assert!(is_downloaded(&data));

        // Losing a file invalidates it again even with the marker present.
        std::fs::remove_file(dir.join(FILES[0].1)).unwrap();
        assert!(!is_downloaded(&data));

        let _ = std::fs::remove_dir_all(&data);
    }

    /// The pinned digests must be well-formed, or verification silently never
    /// matches and every start re-downloads 150MB.
    #[test]
    fn pinned_digests_are_well_formed() {
        assert_eq!(MODEL_REVISION.len(), 40, "revision should be a git SHA-1");
        assert!(MODEL_REVISION.chars().all(|c| c.is_ascii_hexdigit()));
        let total: u64 = FILES.iter().map(|(_, _, _, size)| size).sum();
        assert_eq!(total, DOWNLOAD_BYTES, "progress total must match the parts");
        for (remote, _, sha, size) in FILES {
            assert_eq!(sha.len(), 64, "{remote}: sha256 is 64 hex chars");
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()), "{remote}");
            assert!(size > 0, "{remote}");
        }
    }

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

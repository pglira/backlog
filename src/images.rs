//! Images in the text fields: read them from the clipboard or from a pasted
//! file path, store them next to the backlog file, and draw them with a
//! terminal graphics protocol.
//!
//! A field refers to an image with a Markdown link such as
//! `![Image #1](.backlog/images/0123456789abcdef.png)`. The path is relative
//! to the directory of the backlog file. The file name is a hash of the
//! content, so that the same image is stored once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use image::DynamicImage;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect, Size};
use ratatui_image::FontSize;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::sliced::{SignedPosition, SlicedImage, SlicedProtocol};

/// The directory of the images, relative to the directory of the backlog file.
pub const DIR: &str = ".backlog/images";
/// The image types that the clipboard can give, with their file extensions,
/// in the order of preference.
const MIME_TYPES: [(&str, &str); 4] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
];
const EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
/// The width of an image in cells. Images are previews; the agent reads the
/// files.
pub const WIDTH: u16 = 20;
/// The maximum number of prepared images in the cache.
const CACHE_SIZE: usize = 64;

/// Read an image from the clipboard with `wl-paste` or `xclip`. Return its
/// data and its file extension, or `None` if the clipboard holds no image.
pub fn from_clipboard() -> Option<(Vec<u8>, &'static str)> {
    let tools: [(&str, &[&str], &[&str], &str); 2] = [
        (
            "wl-paste",
            &["--list-types"],
            &["--type"],
            "WAYLAND_DISPLAY",
        ),
        (
            "xclip",
            &["-selection", "clipboard", "-t", "TARGETS", "-o"],
            &["-selection", "clipboard", "-o", "-t"],
            "DISPLAY",
        ),
    ];
    for (cmd, list, read, display) in tools {
        if env::var_os(display).is_none() {
            continue;
        }
        let Some(types) = run(cmd, list) else {
            continue;
        };
        let types = String::from_utf8_lossy(&types);
        let (mime, ext) = MIME_TYPES
            .iter()
            .find(|(mime, _)| types.lines().any(|t| t.trim() == *mime))?;
        let mut args = read.to_vec();
        args.push(mime);
        return run(cmd, &args).filter(|d| !d.is_empty()).map(|d| (d, *ext));
    }
    None
}

/// Run `cmd` and return its output if it succeeds.
fn run(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// Read the image file if `text` is only the path of one, as a terminal
/// pastes it for a dropped file. Accept quotes, a `file://` prefix and
/// backslash-escaped spaces. Return the data and the file extension.
pub fn from_pasted_path(text: &str) -> Option<(Vec<u8>, String)> {
    let text = text.trim();
    if text.is_empty() || text.contains('\n') {
        return None;
    }
    let text = text
        .strip_prefix('\'')
        .and_then(|t| t.strip_suffix('\''))
        .or_else(|| text.strip_prefix('"').and_then(|t| t.strip_suffix('"')))
        .unwrap_or(text);
    let text = text.strip_prefix("file://").unwrap_or(text);
    let path = PathBuf::from(text.replace("\\ ", " "));
    let ext = path.extension()?.to_str()?.to_lowercase();
    if !EXTENSIONS.contains(&ext.as_str()) || !path.is_file() {
        return None;
    }
    Some((fs::read(&path).ok()?, ext))
}

/// Write the image `data` into the image directory below `base` and return
/// its path relative to `base`. An image with the same content is written once.
pub fn store(base: &Path, data: &[u8], ext: &str) -> io::Result<String> {
    let rel = format!("{DIR}/{:016x}.{ext}", fnv1a(data));
    let path = base.join(&rel);
    if !path.exists() {
        fs::create_dir_all(base.join(DIR))?;
        fs::write(&path, data)?;
    }
    Ok(rel)
}

/// The 64-bit FNV-1a hash of `data`.
fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

/// The highest number `n` of the images `![Image #n](...)` in `texts`.
pub fn last_number<'a>(texts: impl IntoIterator<Item = &'a str>) -> u32 {
    texts
        .into_iter()
        .flat_map(|t| t.split("![Image #").skip(1))
        .filter_map(|rest| rest.split(']').next()?.parse().ok())
        .max()
        .unwrap_or(0)
}

/// The path and the width in cells of a prepared image.
type Key = (String, u16);

/// Decoded images and their prepared form for the graphics protocol of the
/// terminal.
pub struct Images {
    picker: Picker,
    /// The directory that relative image paths start from.
    base: PathBuf,
    decoded: RefCell<HashMap<String, Option<DynamicImage>>>,
    /// The prepared images for each path and width.
    prepared: RefCell<HashMap<Key, Option<SlicedProtocol>>>,
    /// The images of the current frame that carry escape sequences in a
    /// cell: the key, the position and the content of the cell.
    sent: RefCell<Vec<(Key, Position, String)>>,
}

impl Images {
    pub fn new(picker: Picker, base: PathBuf) -> Images {
        Images {
            picker,
            base,
            decoded: RefCell::default(),
            prepared: RefCell::default(),
            sent: RefCell::default(),
        }
    }

    /// Forget the prepared images, so that the next draw sends them to the
    /// terminal again.
    pub fn clear(&self) {
        self.prepared.borrow_mut().clear();
    }

    /// The size in cells of the image at `url`, scaled to `width` cells with
    /// its aspect ratio. Return `None` if the image cannot be read.
    pub fn size(&self, url: &str, width: u16) -> Option<Size> {
        self.with(url, width, |p| p.size())
    }

    /// Draw the image at `url`, scaled to `width` cells, at `pos` relative
    /// to `area`. The parts outside of `area` are cut off.
    pub fn draw(&self, f: &mut Frame, url: &str, width: u16, area: Rect, pos: SignedPosition) {
        self.with(url, width, |p| {
            f.render_widget(SlicedImage::new(p, pos), area);
        });
        // The first visible cell of the image carries the escape sequences
        // that send the image data to the terminal, for some protocols only
        // in the first draw.
        let first = Position::new(
            area.x + pos.x.clamp(0, area.width as i16) as u16,
            area.y + pos.y.clamp(0, area.height as i16) as u16,
        );
        if let Some(cell) = f.buffer_mut().cell(first)
            && cell.symbol().starts_with('\x1b')
        {
            let key = (url.to_string(), width);
            let symbol = cell.symbol().to_string();
            self.sent.borrow_mut().push((key, first, symbol));
        }
    }

    /// Forget the prepared images whose escape sequences another widget
    /// drew over in `buf`, so that the next draw sends them again.
    pub fn check_sent(&self, buf: &Buffer) {
        let mut prepared = self.prepared.borrow_mut();
        for (key, pos, symbol) in self.sent.borrow_mut().drain(..) {
            if buf.cell(pos).is_none_or(|c| c.symbol() != symbol) {
                prepared.remove(&key);
            }
        }
    }

    fn with<T>(
        &self,
        url: &str,
        width: u16,
        use_it: impl FnOnce(&SlicedProtocol) -> T,
    ) -> Option<T> {
        let key = (url.to_string(), width);
        let mut prepared = self.prepared.borrow_mut();
        if !prepared.contains_key(&key) {
            if prepared.len() >= CACHE_SIZE {
                prepared.clear();
            }
            let protocol = self.decode(url).and_then(|img| {
                let size = scaled_size(img.width(), img.height(), self.picker.font_size(), width);
                SlicedProtocol::new_with_resize(&self.picker, img, size, Resize::Scale(None)).ok()
            });
            prepared.insert(key.clone(), protocol);
        }
        prepared[&key].as_ref().map(use_it)
    }

    fn decode(&self, url: &str) -> Option<DynamicImage> {
        self.decoded
            .borrow_mut()
            .entry(url.to_string())
            .or_insert_with(|| {
                let path = self.base.join(url);
                image::ImageReader::open(path)
                    .ok()?
                    .with_guessed_format()
                    .ok()?
                    .decode()
                    .ok()
            })
            .clone()
    }
}

/// The size in cells of a `w` x `h` pixel image scaled to `width` cells
/// with its aspect ratio, at least one row high.
fn scaled_size(w: u32, h: u32, font: FontSize, width: u16) -> Size {
    let px_width = width as f64 * font.width as f64;
    let px_height = px_width * h as f64 / w.max(1) as f64;
    let rows = (px_height / font.height as f64)
        .ceil()
        .clamp(1.0, u16::MAX as f64);
    Size::new(width, rows as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling() {
        let font = FontSize::new(10, 20);
        assert_eq!(scaled_size(1920, 1080, font, 20), Size::new(20, 6));
        assert_eq!(scaled_size(10, 10, font, 20), Size::new(20, 10));
        assert_eq!(scaled_size(1000, 1, font, 20), Size::new(20, 1));
    }

    #[test]
    fn numbers() {
        let texts = [
            "a ![Image #2](x.png) b ![Image #10](y.png)",
            "![Image #x](z)",
            "",
        ];
        assert_eq!(last_number(texts), 10);
        assert_eq!(last_number([""]), 0);
    }

    #[test]
    fn pasted_paths() {
        let dir = env::temp_dir().join(format!("backlog-images-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a b.PNG");
        fs::write(&file, b"data").unwrap();
        let path = file.display().to_string();

        let quoted = format!("'{path}'\n");
        let url = format!("file://{}", path.replace(' ', "\\ "));
        let found = [path.as_str(), quoted.as_str(), url.as_str()].map(from_pasted_path);
        let missing = from_pasted_path(&format!("{path}.txt"));
        let text = from_pasted_path("hello");
        let stored = store(&dir, b"data", "png").unwrap();
        fs::remove_dir_all(&dir).unwrap();

        for f in found {
            assert_eq!(f, Some((b"data".to_vec(), "png".to_string())));
        }
        assert_eq!(missing, None);
        assert_eq!(text, None);
        assert_eq!(stored, format!("{DIR}/{:016x}.png", fnv1a(b"data")));
    }
}

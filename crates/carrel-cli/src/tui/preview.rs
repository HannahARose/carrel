//! Item previews beside the card in Browse, in terminals that draw images (#258;
//! `docs/design/mac-app.md`, "Previews").
//!
//! At start-up the terminal is recognised from its environment: the Kitty graphics protocol
//! (Kitty, `WezTerm`, Ghostty), iTerm2's inline images, or Sixel (foot, mlterm, Konsole, Windows
//! Terminal, contour). Anywhere else, or inside tmux or screen, which don't pass images on,
//! nothing changes. The `previews` preference turns them off, or names a protocol for a
//! terminal that isn't recognised.
//!
//! What to show comes from [`carrel_core::previews::preview`], in a background thread so the
//! card always draws first: a cached thumbnail (the Mac app's, or one made here from a PNG), a
//! drawing of a 3D model, an image as it is, or a folder's listing as text. A restricted item
//! shows none. The picture itself is written to the terminal after each frame by [`Painter`],
//! into the cells the frame left empty for it.

use std::cell::Cell;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use base64::Engine as _;
use carrel_core::archive::Archive;
use carrel_core::previews::{Makers, Preview, PreviewCache, png};
use ratatui::layout::Rect;

use super::app::App;
use super::item::ItemView;

/// The largest picture file sent to the terminal as it is, rather than as a thumbnail.
const MOST_BYTES: u64 = 8 * 1024 * 1024;

/// How a terminal draws images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// The Kitty graphics protocol: Kitty, `WezTerm` and Ghostty.
    Kitty,
    /// iTerm2's inline images, which `WezTerm` also reads.
    Iterm2,
    /// Sixel: foot, mlterm, Konsole, Windows Terminal and others.
    Sixel,
}

/// The protocol to draw previews with, from the `previews` preference and the environment
/// (`env` looks a variable up): `None` when previews are off or the terminal isn't known to
/// draw images.
pub fn detect(setting: Option<&str>, env: impl Fn(&str) -> Option<String>) -> Option<Protocol> {
    match setting.map(str::to_lowercase).as_deref() {
        Some("off") => return None,
        Some("kitty") => return Some(Protocol::Kitty),
        Some("iterm2") => return Some(Protocol::Iterm2),
        Some("sixel") => return Some(Protocol::Sixel),
        _ => {}
    }
    let set = |key: &str| env(key).is_some_and(|v| !v.is_empty());
    // tmux and screen keep images to themselves unless they're told to pass them on.
    if set("TMUX") || set("STY") {
        return None;
    }
    let term = env("TERM").unwrap_or_default().to_lowercase();
    let program = env("TERM_PROGRAM").unwrap_or_default().to_lowercase();
    if term.contains("kitty")
        || set("KITTY_WINDOW_ID")
        || term.contains("ghostty")
        || program == "ghostty"
        || program == "wezterm"
        || set("WEZTERM_EXECUTABLE")
    {
        return Some(Protocol::Kitty);
    }
    if program == "iterm.app" || env("LC_TERMINAL").is_some_and(|t| t == "iTerm2") {
        return Some(Protocol::Iterm2);
    }
    let konsole = env("KONSOLE_VERSION")
        .and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|v| v >= 220_400);
    if term.starts_with("foot")
        || term.contains("mlterm")
        || term.contains("sixel")
        || program == "contour"
        || konsole
        || set("WT_SESSION")
    {
        return Some(Protocol::Sixel);
    }
    None
}

/// A preview, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ready {
    /// A picture, as the bytes sent to the terminal.
    Picture(Picture),
    /// A folder's contents.
    Listing(Vec<String>),
    /// Nothing to show: the card has the room to itself.
    Nothing,
}

/// A picture's file, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Where it came from, so a changed picture is drawn again.
    pub path: PathBuf,
    /// The file's bytes: a PNG, or for iTerm2 any image.
    pub bytes: Vec<u8>,
    /// Its size in pixels, when it's a PNG.
    pub size: Option<(u32, u32)>,
    /// Its pixels, for Sixel.
    pub pixels: Option<png::Pixels>,
}

/// Previews in Browse: the protocol found at start-up, and the chosen item's preview.
#[derive(Default)]
pub struct Previews {
    /// How the terminal draws images; `None` shows no previews at all.
    pub protocol: Option<Protocol>,
    /// The card whose preview is shown or being made, and the preview once it's ready.
    current: Option<(String, Option<Ready>)>,
    /// Whether the current card was restricted when its preview was asked for.
    restricted: bool,
    /// The preview being made in the background.
    making: Option<Receiver<Ready>>,
    /// Where the last frame left room for the picture, if it did.
    pub room: Cell<Option<Rect>>,
}

impl Previews {
    /// Previews drawn with `protocol`, or none.
    pub fn with(protocol: Option<Protocol>) -> Self {
        Self {
            protocol,
            ..Self::default()
        }
    }

    /// Previews as this device's preference and this terminal allow.
    pub fn detected() -> Self {
        let setting = crate::config::Prefs::load().ok().and_then(|p| p.previews);
        Self::with(detect(setting.as_deref(), |key| std::env::var(key).ok()))
    }

    /// Starts making the preview of the item `view` shows, unless it's the one already shown.
    pub fn want(&mut self, archive: &Archive, view: &ItemView) {
        let Some(protocol) = self.protocol else {
            return;
        };
        let Some(card) = &view.card else {
            self.current = None;
            self.making = None;
            return;
        };
        // Made again when the card changes to restricted or back.
        if self.current.as_ref().is_some_and(|(c, _)| c == card)
            && self.restricted == view.restricted
        {
            return;
        }
        self.restricted = view.restricted;
        self.current = Some((card.clone(), None));
        let (Some(item), false) = (view.item.clone(), view.restricted) else {
            // A restricted item or a card on its own: nothing to make.
            self.current = Some((card.clone(), Some(Ready::Nothing)));
            self.making = None;
            return;
        };
        let root = archive.root().to_owned();
        let cache = PreviewCache::of(archive);
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(make(&root, &item, &cache, protocol));
        });
        self.making = Some(receive);
    }

    /// Takes the preview made in the background, when it's done. Returns whether one came.
    pub fn poll(&mut self) -> bool {
        let Some(making) = &self.making else {
            return false;
        };
        let Ok(ready) = making.try_recv() else {
            return false;
        };
        self.making = None;
        if let Some((_, slot)) = &mut self.current {
            *slot = Some(ready);
        }
        true
    }

    /// The preview of `card`, once it's ready, unless there's nothing to show.
    pub fn shown(&self, card: Option<&str>) -> Option<&Ready> {
        match &self.current {
            Some((c, Some(ready))) if Some(c.as_str()) == card && *ready != Ready::Nothing => {
                Some(ready)
            }
            _ => None,
        }
    }

    /// The picture to draw after this frame, and where.
    fn wanted(&self) -> Option<(&Picture, Rect)> {
        let room = self.room.get()?;
        match &self.current {
            Some((_, Some(Ready::Picture(picture)))) => Some((picture, room)),
            _ => None,
        }
    }
}

/// The preview of the item at `item`, read for `protocol`.
fn make(root: &std::path::Path, item: &str, cache: &PreviewCache, protocol: Protocol) -> Ready {
    // Starting the providers takes a moment, so only for what they draw.
    let registry =
        carrel_core::previews::is_drawn(&root.join(item)).then(|| crate::providers::registry().0);
    let makers = Makers {
        registry: registry.as_ref(),
        thumbnails: true,
    };
    match carrel_core::previews::preview(root, item, false, cache, makers) {
        Preview::Picture { path, png } => read_picture(path, png, protocol),
        Preview::Listing(lines) => Ready::Listing(lines),
        Preview::Nothing => Ready::Nothing,
    }
}

/// The picture at `path` as `protocol` needs it, or nothing when it can't show it.
fn read_picture(path: PathBuf, is_png: bool, protocol: Protocol) -> Ready {
    // Only iTerm2 reads formats other than PNG itself.
    if !is_png && protocol != Protocol::Iterm2 {
        return Ready::Nothing;
    }
    if std::fs::metadata(&path).map_or(true, |m| m.len() > MOST_BYTES) {
        return Ready::Nothing;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return Ready::Nothing;
    };
    let size = png::size(&bytes);
    let pixels = if protocol == Protocol::Sixel {
        match png::decode(&bytes) {
            Some(pixels) => Some(pixels.fit(
                carrel_core::previews::THUMBNAIL,
                carrel_core::previews::THUMBNAIL,
            )),
            None => return Ready::Nothing,
        }
    } else {
        None
    };
    Ready::Picture(Picture {
        path,
        bytes,
        size,
        pixels,
    })
}

/// How the room for a picture divides: `cols` by `rows` cells, each `cell` pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Room {
    /// Its width in cells.
    pub cols: u16,
    /// Its height in cells.
    pub rows: u16,
    /// A cell's width and height in pixels.
    pub cell: (u32, u32),
}

impl Room {
    /// The cells a picture of `size` pixels takes, shrunk to fit and keeping its shape.
    pub fn fit(self, size: (u32, u32)) -> (u16, u16) {
        let (w, h) = (f64::from(size.0.max(1)), f64::from(size.1.max(1)));
        let room_w = f64::from(self.cols) * f64::from(self.cell.0);
        let room_h = f64::from(self.rows) * f64::from(self.cell.1);
        let scale = (room_w / w).min(room_h / h);
        let cells = |pixels: f64, cell: u32, most: u16| {
            // At most the room, so it fits a u16.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let n = (pixels * scale / f64::from(cell)).ceil() as u16;
            n.clamp(1, most.max(1))
        };
        (
            cells(w, self.cell.0, self.cols),
            cells(h, self.cell.1, self.rows),
        )
    }
}

/// `picture` as the escape sequence that draws it in `room`, for `protocol`; `None` when it
/// can't be drawn there.
pub fn escape(protocol: Protocol, picture: &Picture, room: Room) -> Option<String> {
    match protocol {
        Protocol::Kitty => {
            let (cols, rows) = room.fit(picture.size?);
            let data = base64::engine::general_purpose::STANDARD.encode(&picture.bytes);
            let mut out = String::new();
            let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
            for (i, chunk) in chunks.iter().enumerate() {
                let more = u8::from(i + 1 < chunks.len());
                let chunk = std::str::from_utf8(chunk).unwrap_or_default();
                if i == 0 {
                    write!(
                        out,
                        "\x1b_Ga=T,f=100,t=d,q=2,C=1,c={cols},r={rows},m={more};{chunk}\x1b\\"
                    )
                } else {
                    write!(out, "\x1b_Gm={more};{chunk}\x1b\\")
                }
                .ok()?;
            }
            Some(out)
        }
        Protocol::Iterm2 => {
            let (cols, rows) = picture.size.map_or((room.cols, room.rows), |s| room.fit(s));
            Some(format!(
                "\x1b]1337;File=inline=1;size={};width={cols};height={rows};preserveAspectRatio=1:{}\x07",
                picture.bytes.len(),
                base64::engine::general_purpose::STANDARD.encode(&picture.bytes)
            ))
        }
        Protocol::Sixel => {
            let pixels = picture.pixels.as_ref()?;
            let most_w = u32::from(room.cols) * room.cell.0;
            let most_h = u32::from(room.rows) * room.cell.1;
            Some(sixel(&pixels.fit(most_w, most_h)))
        }
    }
}

/// `pixels` as Sixel, in 216 colours, with transparent pixels left alone.
pub fn sixel(pixels: &png::Pixels) -> String {
    let (width, height) = (pixels.width as usize, pixels.height as usize);
    let level = |v: u8| usize::from(v) * 5 / 255 + usize::from(v % 51 > 25);
    let colours: Vec<Option<usize>> = pixels
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            (p[3] >= 128)
                .then(|| level(p[0]).min(5) * 36 + level(p[1]).min(5) * 6 + level(p[2]).min(5))
        })
        .collect();
    let mut out = format!("\x1bP0;1;0q\"1;1;{width};{height}");
    let mut used = [false; 216];
    for c in colours.iter().flatten() {
        used[*c] = true;
    }
    for (c, _) in used.iter().enumerate().filter(|(_, u)| **u) {
        let percent = |l: usize| l * 100 / 5;
        let _ = write!(
            out,
            "#{c};2;{};{};{}",
            percent(c / 36),
            percent(c / 6 % 6),
            percent(c % 6)
        );
    }
    for band in (0..height).step_by(6) {
        let mut first = true;
        for (c, _) in used.iter().enumerate().filter(|(_, u)| **u) {
            let columns: Vec<u8> = (0..width)
                .map(|x| {
                    (0..6)
                        .filter(|dy| {
                            band + dy < height && colours[(band + dy) * width + x] == Some(c)
                        })
                        .fold(0u8, |bits, dy| bits | (1 << dy))
                })
                .collect();
            if columns.iter().all(|b| *b == 0) {
                continue;
            }
            if !first {
                out.push('$');
            }
            first = false;
            let _ = write!(out, "#{c}");
            run_lengths(&columns, &mut out);
        }
        out.push('-');
    }
    out.push_str("\x1b\\");
    out
}

/// Sixel's characters for `columns`, repeats written as `!n`.
fn run_lengths(columns: &[u8], out: &mut String) {
    let mut at = 0;
    while at < columns.len() {
        let bits = columns[at];
        let run = columns[at..].iter().take_while(|b| **b == bits).count();
        let ch = char::from(63 + bits);
        if run > 3 {
            let _ = write!(out, "!{run}{ch}");
        } else {
            out.extend(std::iter::repeat_n(ch, run));
        }
        at += run;
    }
}

/// Writes the chosen item's picture into the room the frame left for it, after each frame,
/// and takes it away when it's gone.
#[derive(Default)]
pub struct Painter {
    /// The picture drawn, and where.
    drawn: Option<(PathBuf, Rect)>,
}

impl Painter {
    /// Brings the terminal's picture up to date with what `app` shows. Returns whether the
    /// screen must be cleared and drawn again, to take a picture away.
    pub fn paint(&mut self, app: &App) -> bool {
        let Some(protocol) = app.previews.protocol else {
            return false;
        };
        let wanted = app.previews.wanted();
        let now = wanted.map(|(p, room)| (p.path.clone(), room));
        if now == self.drawn {
            return false;
        }
        let mut out = std::io::stdout().lock();
        if self.drawn.take().is_some() {
            if protocol == Protocol::Kitty {
                let _ = write!(out, "\x1b_Ga=d,q=2\x1b\\");
                let _ = out.flush();
            }
            // Sixel and iTerm2 pictures are in the cells, which a full redraw replaces.
            return true;
        }
        if let Some((picture, rect)) = wanted {
            let room = Room {
                cols: rect.width,
                rows: rect.height,
                cell: cell_size(),
            };
            if let Some(escape) = escape(protocol, picture, room) {
                let _ = write!(out, "\x1b7\x1b[{};{}H{escape}\x1b8", rect.y + 1, rect.x + 1);
                let _ = out.flush();
            }
            self.drawn = now;
        }
        false
    }
}

/// A cell's size in pixels, as the terminal says, or a usual one when it doesn't.
fn cell_size() -> (u32, u32) {
    ratatui::crossterm::terminal::window_size()
        .ok()
        .filter(|s| s.width > 0 && s.height > 0 && s.columns > 0 && s.rows > 0)
        .map_or((10, 20), |s| {
            (
                u32::from(s.width / s.columns).max(1),
                u32::from(s.height / s.rows).max(1),
            )
        })
}

impl App {
    /// Whether anything is drawn over Browse, so a picture would cover it.
    pub(super) fn covered(&self) -> bool {
        self.help
            || self.picker.is_some()
            || self.browser.is_some()
            || self.input.is_some()
            || self.correction.is_some()
            || self.confirm.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| vars.get(key).cloned()
    }

    #[test]
    fn terminals_that_draw_images_are_recognised() {
        let kitty = Some(Protocol::Kitty);
        assert_eq!(detect(None, env(&[("TERM", "xterm-kitty")])), kitty);
        assert_eq!(detect(None, env(&[("TERM", "xterm-ghostty")])), kitty);
        assert_eq!(detect(None, env(&[("TERM_PROGRAM", "WezTerm")])), kitty);
        assert_eq!(
            detect(None, env(&[("TERM_PROGRAM", "iTerm.app")])),
            Some(Protocol::Iterm2)
        );
        assert_eq!(
            detect(None, env(&[("LC_TERMINAL", "iTerm2")])),
            Some(Protocol::Iterm2)
        );
        assert_eq!(
            detect(None, env(&[("TERM", "foot")])),
            Some(Protocol::Sixel)
        );
        assert_eq!(
            detect(None, env(&[("KONSOLE_VERSION", "230800")])),
            Some(Protocol::Sixel)
        );
        assert_eq!(detect(None, env(&[("KONSOLE_VERSION", "210400")])), None);
    }

    #[test]
    fn other_terminals_and_the_off_switch_show_no_previews() {
        assert_eq!(detect(None, env(&[])), None);
        assert_eq!(detect(None, env(&[("TERM", "xterm-256color")])), None);
        assert_eq!(
            detect(None, env(&[("TERM_PROGRAM", "Apple_Terminal")])),
            None
        );
        // tmux doesn't pass images on.
        assert_eq!(
            detect(
                None,
                env(&[("TERM", "xterm-kitty"), ("TMUX", "/tmp/t,1,0")])
            ),
            None
        );
        assert_eq!(detect(Some("off"), env(&[("TERM", "xterm-kitty")])), None);
        assert_eq!(
            detect(Some("auto"), env(&[("TERM", "xterm-kitty")])),
            Some(Protocol::Kitty)
        );
        // A protocol named for a terminal that isn't recognised.
        assert_eq!(detect(Some("sixel"), env(&[])), Some(Protocol::Sixel));
    }

    fn picture(width: u32, height: u32) -> Picture {
        let pixels = png::Pixels {
            width,
            height,
            rgba: [255, 0, 0, 255].repeat((width * height) as usize),
        };
        let bytes = pixels.encode();
        Picture {
            path: PathBuf::from("red.png"),
            size: Some((width, height)),
            bytes,
            pixels: Some(pixels),
        }
    }

    #[test]
    fn pictures_fit_their_room_keeping_their_shape() {
        let room = Room {
            cols: 40,
            rows: 20,
            cell: (10, 20),
        };
        // 400×400 pixels of room: a wide picture takes the width and a quarter of the height.
        assert_eq!(room.fit((800, 200)), (40, 5));
        assert_eq!(room.fit((100, 400)), (10, 20));
    }

    #[test]
    fn each_protocol_has_its_own_escape() {
        let room = Room {
            cols: 40,
            rows: 20,
            cell: (10, 20),
        };
        let red = picture(8, 8);
        let kitty = escape(Protocol::Kitty, &red, room).unwrap();
        assert!(kitty.starts_with("\x1b_Ga=T,f=100,"), "{kitty:?}");
        assert!(kitty.contains("c=40,r=20"), "{kitty:?}");
        let iterm = escape(Protocol::Iterm2, &red, room).unwrap();
        assert!(iterm.starts_with("\x1b]1337;File=inline=1;"), "{iterm:?}");
        let sixel = escape(Protocol::Sixel, &red, room).unwrap();
        // Red, then eight columns of six pixels, then two.
        assert_eq!(
            sixel,
            "\x1bP0;1;0q\"1;1;8;8#180;2;100;0;0#180!8~-#180!8B-\x1b\\"
        );
        // Kitty needs a PNG's size; Sixel its pixels.
        let other = Picture {
            size: None,
            pixels: None,
            ..red
        };
        assert_eq!(escape(Protocol::Kitty, &other, room), None);
        assert_eq!(escape(Protocol::Sixel, &other, room), None);
    }

    /// Shows the item whose card is `card` in Browse, and waits for its preview.
    fn browse_to(app: &mut App, card: &str) {
        app.tab = super::super::app::Tab::Browse;
        for i in 0..app.collections.len() {
            app.browse_collection = i;
            app.load_items();
            if let Some(at) = app.browse_items.iter().position(|(c, _)| c == card) {
                app.browse_item = at;
                app.load_card();
                let started = std::time::Instant::now();
                while app.previews.making.is_some() && started.elapsed().as_secs() < 10 {
                    app.previews.poll();
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                return;
            }
        }
        panic!("{card} isn't listed");
    }

    #[test]
    fn browse_shows_previews_beside_the_card_where_the_terminal_draws_them() {
        let (dir, mut app) = super::super::tests::sample(false);
        let root = dir.path().join("archive");
        app.previews = Previews::with(Some(Protocol::Kitty));

        // A folder's preview is its listing, on any terminal that draws images.
        browse_to(&mut app, "Projects/Hardware/lamp-build.md");
        let screen = super::super::tests::screen(&app, 140, 30);
        assert!(screen.contains("Preview"), "{screen}");
        assert!(
            screen.contains("arm.svg") && screen.contains("notes.txt"),
            "{screen}"
        );
        assert_eq!(app.previews.room.get(), None, "a listing is text");

        // A picture gets the room beside the card, from a thumbnail made in the background.
        // The file's contents say it's a PNG, whatever its name.
        let picture = png::Pixels {
            width: 1200,
            height: 600,
            rgba: [0, 128, 255, 255].repeat(1200 * 600),
        };
        std::fs::write(
            root.join("Projects/Hardware/parts-list.csv"),
            picture.encode(),
        )
        .unwrap();
        browse_to(&mut app, "Projects/Hardware/parts-list.csv.md");
        let screen = super::super::tests::screen(&app, 140, 30);
        assert!(screen.contains("Preview"), "{screen}");
        let space = app.previews.room.get().expect("room for the picture");
        assert!(space.width > 20 && space.height > 10, "{space:?}");
        let Some(Ready::Picture(shown)) = app.previews.shown(app.item.card.as_deref()) else {
            panic!("no picture");
        };
        assert!(
            shown
                .path
                .starts_with(root.join(carrel_core::previews::PREVIEWS))
        );
        assert_eq!(shown.size, Some((512, 256)));
        // A pop-up over Browse takes the picture away, so it doesn't cover the pop-up.
        app.help = true;
        let _ = super::super::tests::screen(&app, 140, 30);
        assert_eq!(app.previews.room.get(), None);
        app.help = false;

        // A restricted item shows its card only.
        let card = root.join("Projects/Hardware/lamp-build.md");
        let text = std::fs::read_to_string(&card).unwrap();
        std::fs::write(
            &card,
            text.replace("sensitivity: personal", "sensitivity: restricted"),
        )
        .unwrap();
        browse_to(&mut app, "Projects/Hardware/lamp-build.md");
        assert!(app.item.restricted);
        let screen = super::super::tests::screen(&app, 140, 30);
        assert!(!screen.contains("Preview"), "{screen}");
        assert!(!screen.contains("arm.svg"), "{screen}");
    }

    #[test]
    fn terminals_without_images_show_browse_as_before() {
        let (_dir, mut app) = super::super::tests::sample(false);
        assert_eq!(app.previews.protocol, None);
        browse_to(&mut app, "Projects/Hardware/lamp-build.md");
        assert!(app.previews.current.is_none(), "nothing is made");
        let screen = super::super::tests::screen(&app, 140, 30);
        assert!(!screen.contains("Preview"), "{screen}");
        assert!(!screen.contains("arm.svg"), "{screen}");
        assert_eq!(app.previews.room.get(), None);
    }

    #[test]
    fn only_iterm2_is_sent_images_other_than_pngs() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.jpg");
        std::fs::write(&photo, b"\xff\xd8\xff\xe0").unwrap();
        assert_eq!(
            read_picture(photo.clone(), false, Protocol::Kitty),
            Ready::Nothing
        );
        assert_eq!(
            read_picture(photo.clone(), false, Protocol::Sixel),
            Ready::Nothing
        );
        let Ready::Picture(read) = read_picture(photo, false, Protocol::Iterm2) else {
            panic!("iTerm2 reads JPEGs");
        };
        assert_eq!(read.size, None);
    }
}

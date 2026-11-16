//! Win32 boundary: HWNDs stay on the UI thread, callbacks borrow through RefCell
//! to tolerate synchronous control notifications. Pixel staging runs off-thread.
use ncat_backend::{Backend, BackendOptions, Book, Event, Frame, View, WorkerCommand};
use ncat_presentation::{
    ReaderView, adjacent,
    input::{self, Command, Key, Modifiers},
    interface_shown,
    playback::{Playback, SPEEDS},
};
use std::{
    cell::RefCell,
    io,
    path::PathBuf,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::{Com::*, LibraryLoader::GetModuleHandleW},
    UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const RESULT: u32 = WM_APP + 1;
const ANIMATION: usize = 1;
const SHUTDOWN: usize = 2;
const DEADLINE: usize = 3;
const OPEN: u16 = 101;
const FOLDER: u16 = 102;
const ADDRESS: u16 = 103;
const CONNECT: u16 = 104;
const PREVIOUS: u16 = 105;
const PAGE: u16 = 106;
const PAGE_GO: u16 = 107;
const NEXT: u16 = 108;
const DIRECTION: u16 = 109;
const SMALLER: u16 = 110;
const FIT: u16 = 111;
const LARGER: u16 = 112;
const ROTATE: u16 = 113;
const FULLSCREEN: u16 = 114;
const FRAME_PREVIOUS: u16 = 115;
const FRAME_NEXT: u16 = 116;
const PLAY: u16 = 117;
const SPEED: u16 = 118;
const FRAME_NUMBER: u16 = 119;
const FRAME_GO: u16 = 120;
const FRAME_INFO: u16 = 121;
const STATUS: u16 = 122;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Smoke {
    None,
    Window,
    Frame,
    Animation,
    Navigation,
}

struct Prepared {
    event: Event,
    pixels: Option<Vec<u8>>,
}
struct Display {
    frame: Arc<Frame>,
    bgra: Vec<u8>,
}

struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: run owns this HWND and drops the guard before its callback state.
        unsafe {
            if IsWindow(self.0) != 0 {
                DestroyWindow(self.0);
            }
        }
    }
}
struct App {
    hwnd: HWND,
    controls: Vec<(u16, HWND, i32, i32, i32)>,
    backend: Backend,
    events: mpsc::Receiver<Prepared>,
    stop: Arc<AtomicBool>,
    book: Option<Arc<Book>>,
    frame: Option<Display>,
    selected: usize,
    reader: ReaderView,
    playback: Playback,
    animation_view: View,
    rtl: bool,
    interface: bool,
    controls_input: bool,
    fullscreen: Option<(isize, RECT)>,
    closing: Option<Instant>,
    drag: Option<(i32, i32)>,
    pan: (i32, i32),
    wheel: f64,
    last_wheel: Option<Instant>,
    smoke: Smoke,
    smoke_frames: u32,
    navigation_left: u32,
    success: bool,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn stage(event: Event) -> Prepared {
    let pixels = if let Event::Frame { frame, .. } = &event {
        let source: &[u8] = frame.as_ref().as_ref();
        let mut bgra = Vec::new();
        if bgra.try_reserve_exact(source.len()).is_err() {
            return Prepared {
                event: Event::Error {
                    generation: event.generation(),
                    code: ncat_protocol::ErrorCode::Limit,
                },
                pixels: None,
            };
        }
        for pixel in source.as_chunks::<4>().0 {
            let alpha = u32::from(pixel[3]);
            let channel =
                |value: u8| ((u32::from(value) * alpha + 32 * (255 - alpha) + 127) / 255) as u8;
            bgra.extend([channel(pixel[2]), channel(pixel[1]), channel(pixel[0]), 255]);
        }
        Some(bgra)
    } else {
        None
    };
    Prepared { event, pixels }
}

impl App {
    fn new(smoke: Smoke) -> io::Result<(Self, mpsc::SyncSender<Prepared>)> {
        let backend = Backend::with_worker_command(
            WorkerCommand::same_executable(std::env::current_exe()?),
            BackendOptions::from_env()?,
        )?;
        let (sender, events) = mpsc::sync_channel(2);
        Ok((
            Self {
                hwnd: ptr::null_mut(),
                controls: Vec::new(),
                backend,
                events,
                stop: Arc::new(AtomicBool::new(false)),
                book: None,
                frame: None,
                selected: 0,
                reader: ReaderView::default(),
                playback: Playback::default(),
                animation_view: View::default(),
                rtl: false,
                interface: true,
                controls_input: false,
                fullscreen: None,
                closing: None,
                drag: None,
                pan: (0, 0),
                wheel: 0.0,
                last_wheel: None,
                smoke,
                smoke_frames: 0,
                navigation_left: if smoke == Smoke::Navigation { 500 } else { 0 },
                success: false,
            },
            sender,
        ))
    }
    fn control(&self, id: u16) -> HWND {
        self.controls
            .iter()
            .find(|entry| entry.0 == id)
            .map_or(ptr::null_mut(), |entry| entry.1)
    }
    unsafe fn text(&self, id: u16, text: &str) {
        // SAFETY: controls belong to this UI thread, and the buffer lives through the call.
        unsafe {
            SetWindowTextW(self.control(id), wide(text).as_ptr());
        }
    }
    unsafe fn value(&self, id: u16) -> String {
        unsafe {
            let hwnd = self.control(id);
            let len = GetWindowTextLengthW(hwnd).clamp(0, 16384) as usize;
            let mut buffer = vec![0; len + 1];
            let read =
                GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32).max(0) as usize;
            String::from_utf16_lossy(&buffer[..read])
        }
    }
    unsafe fn viewport(&self) -> RECT {
        unsafe {
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(self.hwnd, &mut rect);
            if interface_shown(self.interface, self.fullscreen.is_some()) {
                rect.top = 104;
            }
            rect
        }
    }
    unsafe fn view(&self) -> View {
        unsafe {
            let rect = self.viewport();
            let (width, height) = self.reader.render_size(
                (rect.right, rect.bottom - rect.top),
                1.0,
                ncat_protocol::MAX_VIEW_DIMENSION,
            );
            View {
                width,
                height,
                quarter_turns: self.reader.quarter_turns,
            }
        }
    }
    unsafe fn layout(&self) {
        unsafe {
            let show = interface_shown(self.interface, self.fullscreen.is_some());
            for &(_, hwnd, x, y, width) in &self.controls {
                ShowWindow(hwnd, if show { SW_SHOW } else { SW_HIDE });
                MoveWindow(
                    hwnd,
                    x,
                    y,
                    width,
                    if hwnd == self.control(SPEED) { 280 } else { 28 },
                    1,
                );
            }
            InvalidateRect(self.hwnd, ptr::null(), 0);
        }
    }
    unsafe fn playback_ui(&self) {
        unsafe {
            let p = self.playback;
            for id in [FRAME_PREVIOUS, FRAME_NEXT, PLAY, SPEED] {
                EnableWindow(self.control(id), i32::from(p.active));
            }
            for id in [FRAME_NUMBER, FRAME_GO] {
                EnableWindow(self.control(id), i32::from(p.can_jump()));
            }
            self.text(
                PLAY,
                if p.paused {
                    "재생 /"
                } else {
                    "일시정지 /"
                },
            );
            self.text(
                FRAME_INFO,
                &if p.active {
                    format!(
                        "프레임 {}/{} · {}",
                        p.index + 1,
                        p.total.map_or("?".into(), |n| n.to_string()),
                        if p.ended {
                            "종료"
                        } else if p.paused {
                            "일시정지"
                        } else {
                            "재생"
                        }
                    )
                } else {
                    "애니메이션 없음".into()
                },
            );
        }
    }
    unsafe fn navigation_ui(&self) {
        unsafe {
            let len = self
                .book
                .as_ref()
                .filter(|book| book.complete)
                .map_or(0, |book| book.pages.len());
            self.text(PAGE, &(self.selected + 1).to_string());
            EnableWindow(
                self.control(PREVIOUS),
                i32::from(len > 0 && self.selected > 0),
            );
            EnableWindow(
                self.control(NEXT),
                i32::from(len > 0 && self.selected + 1 < len),
            );
            for id in [PAGE, PAGE_GO] {
                EnableWindow(self.control(id), i32::from(len > 0));
            }
            if let Some(book) = &self.book {
                let name = book
                    .pages
                    .get(self.selected)
                    .map_or("", |page| page.name.as_str());
                let title = format!(
                    "{} / {} · {} — nCatImage v{} [UNTESTED Windows]",
                    self.selected + 1,
                    if book.complete {
                        book.pages.len().to_string()
                    } else {
                        "분석 중".into()
                    },
                    name,
                    env!("CARGO_PKG_VERSION")
                );
                SetWindowTextW(self.hwnd, wide(&title).as_ptr());
            }
        }
    }
    unsafe fn reset(&mut self, autoplay: bool) {
        unsafe {
            KillTimer(self.hwnd, ANIMATION);
            self.playback = Playback {
                speed: self.playback.speed,
                paused: !autoplay && self.playback.paused,
                ..Playback::default()
            };
            self.playback_ui();
        }
    }
    unsafe fn open(&mut self, path: PathBuf) {
        unsafe {
            self.reset(true);
            self.book = None;
            self.frame = None;
            self.selected = 0;
            self.reader = ReaderView::default();
            self.pan = (0, 0);
            self.animation_view = self.view();
            if let Err(code) = self.backend.open(path, self.animation_view) {
                self.text(STATUS, &code.to_string());
            } else {
                self.text(STATUS, "읽는 중…");
            }
            self.navigation_ui();
            InvalidateRect(self.hwnd, ptr::null(), 0);
            self.controls_input = false;
            SetFocus(self.hwnd);
        }
    }
    unsafe fn load(&mut self, index: usize) {
        unsafe {
            let Some(book) = self
                .book
                .clone()
                .filter(|b| b.complete && index < b.pages.len())
            else {
                return;
            };
            self.reset(index != self.selected);
            self.selected = index;
            self.animation_view = self.view();
            if self.backend.load(book, index, self.animation_view).is_ok() {
                self.text(STATUS, "읽는 중 · 이전 페이지 표시 중");
            }
            self.navigation_ui();
        }
    }
    unsafe fn navigate(&mut self, forward: bool) {
        unsafe {
            let len = self
                .book
                .as_ref()
                .filter(|b| b.complete)
                .map_or(0, |b| b.pages.len());
            if let Some(index) = adjacent(self.selected, len, forward)
                && index != self.selected
            {
                self.reader.zoom = 1.0;
                self.reader.quarter_turns = 0;
                self.pan = (0, 0);
                self.load(index);
            }
        }
    }
    unsafe fn animation(&mut self, seek: Option<u32>) {
        unsafe {
            if !self.playback.active || self.playback.busy {
                return;
            }
            let Some(book) = self.book.clone() else {
                return;
            };
            KillTimer(self.hwnd, ANIMATION);
            let result = if let Some(index) = seek {
                self.backend
                    .animation_seek(book, self.selected, self.animation_view, index)
            } else {
                self.backend
                    .animation_next(book, self.selected, self.animation_view)
            };
            self.playback.busy = result.is_ok();
            self.playback_ui();
        }
    }
    unsafe fn schedule(&self) {
        unsafe {
            KillTimer(self.hwnd, ANIMATION);
            if self.playback.active
                && !self.playback.paused
                && !self.playback.ended
                && !self.playback.busy
            {
                SetTimer(self.hwnd, ANIMATION, self.playback.delay() as u32, None);
            }
        }
    }
    unsafe fn step(&mut self, forward: bool) {
        unsafe {
            if !self.playback.active {
                return;
            }
            self.playback.paused = true;
            KillTimer(self.hwnd, ANIMATION);
            self.playback_ui();
            if self.playback.busy {
                return;
            }
            if forward && !self.playback.ended {
                self.animation(None);
            } else if !forward && self.playback.index > 0 {
                self.animation(Some(self.playback.index - 1));
            }
        }
    }
    unsafe fn play(&mut self) {
        unsafe {
            if !self.playback.active {
                return;
            }
            self.playback.paused = !self.playback.paused;
            self.playback_ui();
            if !self.playback.paused && self.playback.ended {
                self.animation(Some(0));
            } else {
                self.schedule();
            }
        }
    }
    unsafe fn zoom(&mut self, factor: f64) {
        unsafe {
            if self.book.is_none() {
                return;
            }
            self.reader.change_zoom(factor);
            self.pan = (0, 0);
            self.load(self.selected);
        }
    }
    unsafe fn fullscreen(&mut self) {
        unsafe {
            if let Some((style, rect)) = self.fullscreen.take() {
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, style);
                SetWindowPos(
                    self.hwnd,
                    ptr::null_mut(),
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_FRAMECHANGED,
                );
            } else {
                let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
                let mut rect: RECT = std::mem::zeroed();
                GetWindowRect(self.hwnd, &mut rect);
                let mut monitor: MONITORINFO = std::mem::zeroed();
                monitor.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                if GetMonitorInfoW(
                    MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST),
                    &mut monitor,
                ) == 0
                {
                    return;
                }
                self.fullscreen = Some((style, rect));
                SetWindowLongPtrW(
                    self.hwnd,
                    GWL_STYLE,
                    style & !(WS_OVERLAPPEDWINDOW as isize),
                );
                let r = monitor.rcMonitor;
                SetWindowPos(
                    self.hwnd,
                    ptr::null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_FRAMECHANGED,
                );
            }
            self.controls_input = false;
            SetFocus(self.hwnd);
            self.layout();
        }
    }
    unsafe fn consume(&mut self, prepared: Prepared) {
        unsafe {
            if self.closing.is_some() || prepared.event.generation() != self.backend.generation() {
                return;
            }
            match prepared.event {
                Event::Catalog { book, .. } => {
                    self.selected = book.selected;
                    self.book = Some(book);
                    self.navigation_ui();
                }
                Event::Frame { frame, .. } => {
                    let Some(pixels) = prepared.pixels else {
                        return;
                    };
                    self.reader.original = (frame.info.original_width, frame.info.original_height);
                    if let Some(animation) = &frame.info.animation {
                        self.playback.displayed(
                            animation.index,
                            animation.cycle,
                            animation.delay_ms,
                        );
                        self.playback.total = animation.total.or(self.playback.total);
                        self.text(FRAME_NUMBER, &(animation.index + 1).to_string());
                    } else {
                        self.reset(true);
                    }
                    self.text(
                        STATUS,
                        &format!(
                            "{} · {}×{} · {:.1} MiB 예약",
                            frame.info.format,
                            frame.info.original_width,
                            frame.info.original_height,
                            self.backend.budget_used() as f64 / 1048576.0
                        ),
                    );
                    let animated = frame.info.animation.is_some();
                    self.frame = Some(Display {
                        frame,
                        bgra: pixels,
                    });
                    self.playback_ui();
                    self.schedule();
                    InvalidateRect(self.hwnd, ptr::null(), 0);
                    self.smoke_frames += 1;
                    if self.smoke == Smoke::Frame
                        || (self.smoke == Smoke::Animation && animated && self.smoke_frames >= 6)
                    {
                        self.success = true;
                        self.close();
                    }
                }
                Event::AnimationEnded { .. } => {
                    self.playback.finish();
                    self.playback_ui();
                    KillTimer(self.hwnd, ANIMATION);
                    if self.smoke == Smoke::Animation {
                        self.success = self.smoke_frames >= 2;
                        self.close();
                    }
                }
                Event::Error { code, .. } => {
                    self.reset(true);
                    self.frame = None;
                    self.text(STATUS, &code.to_string());
                    InvalidateRect(self.hwnd, ptr::null(), 0);
                    if self.smoke != Smoke::None {
                        eprintln!("nCatImage: GUI smoke error={code:?}");
                        self.close();
                    }
                }
            }
            if self.smoke == Smoke::Navigation && self.frame.is_some() {
                let len = self
                    .book
                    .as_ref()
                    .filter(|b| b.complete)
                    .map_or(0, |b| b.pages.len());
                if len > 0 {
                    if self.navigation_left > 0 {
                        self.navigation_left -= 1;
                        self.load((self.selected + 1) % len);
                    } else {
                        self.success = true;
                        self.close();
                    }
                }
            }
        }
    }
    unsafe fn close(&mut self) {
        unsafe {
            if self.closing.is_some() {
                return;
            }
            self.closing = Some(Instant::now());
            self.stop.store(true, Ordering::Release);
            KillTimer(self.hwnd, ANIMATION);
            self.backend.shutdown();
            SetTimer(self.hwnd, SHUTDOWN, 10, None);
        }
    }
    unsafe fn command(&mut self, id: u16, notification: u16) {
        unsafe {
            if notification == EN_CHANGE as u16 {
                return;
            }
            match id {
                OPEN => {
                    if let Some(path) = file_dialog(self.hwnd) {
                        self.open(path);
                    }
                }
                FOLDER => {
                    if let Some(path) = folder_dialog(self.hwnd) {
                        self.open(path);
                    }
                }
                CONNECT => match crate::address::parse(&self.value(ADDRESS)) {
                    Ok(path) => self.open(path),
                    Err(text) => self.text(STATUS, text),
                },
                PREVIOUS => self.navigate(false),
                NEXT => self.navigate(true),
                PAGE_GO => {
                    if let Ok(number) = self.value(PAGE).trim().parse::<usize>()
                        && number > 0
                    {
                        self.load(number - 1);
                    }
                }
                DIRECTION => {
                    self.rtl = !self.rtl;
                    self.text(DIRECTION, if self.rtl { "우→좌" } else { "좌→우" });
                }
                SMALLER => self.zoom(1.0 / 1.25),
                FIT => self.zoom(0.0),
                LARGER => self.zoom(1.25),
                ROTATE => {
                    self.reader.rotate(false);
                    self.load(self.selected);
                }
                FULLSCREEN => self.fullscreen(),
                FRAME_PREVIOUS => self.step(false),
                FRAME_NEXT => self.step(true),
                PLAY => self.play(),
                SPEED if notification == CBN_SELCHANGE as u16 => {
                    let index = SendMessageW(self.control(SPEED), CB_GETCURSEL, 0, 0) as usize;
                    if let Some(speed) = SPEEDS.get(index) {
                        self.playback.set_speed(*speed);
                        self.schedule();
                    }
                }
                FRAME_GO if self.playback.can_jump() => {
                    if let Ok(number) = self.value(FRAME_NUMBER).trim().parse::<u32>()
                        && (1..=ncat_protocol::MAX_ANIMATION_FRAMES).contains(&number)
                    {
                        self.animation(Some(number - 1));
                    }
                }
                _ => {}
            }
        }
    }
    unsafe fn key(&mut self, key: u32) -> bool {
        unsafe {
            let modifiers = Modifiers {
                primary: GetKeyState(VK_CONTROL as i32) < 0,
                shift: GetKeyState(VK_SHIFT as i32) < 0,
                alternate: GetKeyState(VK_MENU as i32) < 0,
                system: GetKeyState(VK_LWIN as i32) < 0 || GetKeyState(VK_RWIN as i32) < 0,
            };
            if key == VK_RETURN as u32 {
                let focus = GetFocus();
                for (id, action) in [
                    (PAGE, PAGE_GO),
                    (FRAME_NUMBER, FRAME_GO),
                    (ADDRESS, CONNECT),
                ] {
                    if focus == self.control(id) {
                        self.command(action, 0);
                        return true;
                    }
                }
            }
            let key = match key as u16 {
                VK_OEM_COMMA => Key::FramePrevious,
                VK_OEM_PERIOD => Key::FrameNext,
                VK_OEM_2 => Key::Playback,
                VK_F11 => Key::Fullscreen,
                VK_ESCAPE => Key::Escape,
                VK_OEM_3 => Key::ToggleInterface,
                VK_TAB => Key::Tab,
                VK_DOWN | VK_NEXT | VK_SPACE => Key::Next,
                VK_UP | VK_PRIOR | VK_BACK => Key::Previous,
                VK_LEFT => Key::Left,
                VK_RIGHT => Key::Right,
                VK_HOME => Key::First,
                VK_END => Key::Last,
                VK_OEM_PLUS | VK_ADD => Key::ZoomIn,
                VK_OEM_MINUS | VK_SUBTRACT => Key::ZoomOut,
                VK_F5 => Key::Reload,
                0x4f => Key::Open,
                0x4c => Key::Network,
                0x51 => Key::Quit,
                0x52 => Key::Rotate,
                0x44 => Key::Direction,
                0x39 => Key::Fit,
                _ => Key::Other,
            };
            match input::command(
                key,
                modifiers,
                self.controls_input && GetFocus() != self.hwnd,
            ) {
                Command::Native => return false,
                Command::Block => {}
                Command::Open { folder } => self.command(if folder { FOLDER } else { OPEN }, 0),
                Command::Network => {
                    if self.fullscreen.is_some() {
                        self.fullscreen();
                    }
                    self.interface = true;
                    self.layout();
                    self.controls_input = true;
                    SetFocus(self.control(ADDRESS));
                }
                Command::Quit => self.close(),
                Command::Fullscreen => self.fullscreen(),
                Command::ToggleInterface => {
                    if self.fullscreen.is_none() {
                        self.interface = !self.interface;
                        self.layout();
                        SetFocus(self.hwnd);
                        self.controls_input = false;
                    }
                }
                Command::Page(key) => self.navigate(ncat_presentation::page_step(
                    key,
                    if self.rtl {
                        ncat_core::Direction::RightToLeft
                    } else {
                        ncat_core::Direction::LeftToRight
                    },
                )),
                Command::First => self.load(0),
                Command::Last => {
                    if let Some(book) = &self.book {
                        self.load(book.pages.len().saturating_sub(1));
                    }
                }
                Command::ZoomIn => self.zoom(1.25),
                Command::ZoomOut => self.zoom(1.0 / 1.25),
                Command::Fit => self.zoom(0.0),
                Command::Rotate { backwards } => {
                    self.reader.rotate(backwards);
                    self.load(self.selected);
                }
                Command::Direction => self.command(DIRECTION, 0),
                Command::Reload => {
                    if let Some(book) = &self.book {
                        self.open(book.source.path());
                    }
                }
                Command::FramePrevious => self.step(false),
                Command::FrameNext => self.step(true),
                Command::Playback => self.play(),
            }
            true
        }
    }
    unsafe fn paint(&mut self) {
        unsafe {
            let mut paint: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(self.hwnd, &mut paint);
            if !dc.is_null() {
                let rect = self.viewport();
                IntersectClipRect(dc, rect.left, rect.top, rect.right, rect.bottom);
                let brush = CreateSolidBrush(0x202020);
                FillRect(dc, &rect, brush);
                DeleteObject(brush);
                if let Some(display) = &self.frame {
                    let width = display.frame.info.width as i32;
                    let height = display.frame.info.height as i32;
                    let (w, h) = self
                        .reader
                        .picture_size((rect.right, rect.bottom - rect.top))
                        .unwrap_or_else(|| {
                            let (w, h) = ncat_core::fit_size(
                                width as u32,
                                height as u32,
                                rect.right.max(1) as u32,
                                (rect.bottom - rect.top).max(1) as u32,
                            )
                            .unwrap_or((1, 1));
                            (w as i32, h as i32)
                        });
                    self.pan.0 = self
                        .pan
                        .0
                        .clamp(-((w - rect.right).max(0) / 2), (w - rect.right).max(0) / 2);
                    self.pan.1 = self.pan.1.clamp(
                        -((h - (rect.bottom - rect.top)).max(0) / 2),
                        (h - (rect.bottom - rect.top)).max(0) / 2,
                    );
                    let mut bitmap: BITMAPINFO = std::mem::zeroed();
                    bitmap.bmiHeader = BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width,
                        biHeight: -height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB,
                        ..std::mem::zeroed()
                    };
                    SetStretchBltMode(dc, HALFTONE);
                    StretchDIBits(
                        dc,
                        (rect.right - w) / 2 + self.pan.0,
                        rect.top + (rect.bottom - rect.top - h) / 2 + self.pan.1,
                        w,
                        h,
                        0,
                        0,
                        width,
                        height,
                        display.bgra.as_ptr().cast(),
                        &bitmap,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                }
            }
            EndPaint(self.hwnd, &paint);
        }
    }
}

unsafe fn file_dialog(owner: HWND) -> Option<PathBuf> {
    unsafe {
        let mut buffer = vec![0u16; 32768];
        let filter:Vec<u16>="이미지 / ZIP / CBZ\0*.jpg;*.jpeg;*.png;*.apng;*.gif;*.agif;*.webp;*.bmp;*.zip;*.cbz\0모든 파일\0*.*\0\0".encode_utf16().collect();
        let mut dialog: OPENFILENAMEW = std::mem::zeroed();
        dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        dialog.hwndOwner = owner;
        dialog.lpstrFile = buffer.as_mut_ptr();
        dialog.nMaxFile = buffer.len() as u32;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.Flags = OFN_FILEMUSTEXIST
            | OFN_PATHMUSTEXIST
            | OFN_EXPLORER
            | OFN_DONTADDTORECENT
            | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut dialog) == 0 {
            return None;
        }
        use std::os::windows::ffi::OsStringExt;
        let end = buffer.iter().position(|c| *c == 0)?;
        Some(std::ffi::OsString::from_wide(&buffer[..end]).into())
    }
}
unsafe fn folder_dialog(owner: HWND) -> Option<PathBuf> {
    unsafe {
        let title = wide("만화 폴더 열기");
        let info = BROWSEINFOW {
            hwndOwner: owner,
            lpszTitle: title.as_ptr(),
            ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
            ..std::mem::zeroed()
        };
        let item = SHBrowseForFolderW(&info);
        if item.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32768];
        let valid = SHGetPathFromIDListEx(item, buffer.as_mut_ptr(), buffer.len() as u32, 0);
        CoTaskMemFree(item.cast());
        if valid == 0 {
            return None;
        }
        use std::os::windows::ffi::OsStringExt;
        let end = buffer.iter().position(|c| *c == 0)?;
        Some(std::ffi::OsString::from_wide(&buffer[..end]).into())
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: pointer targets the run-owned RefCell, which outlives every HWND.
    // Reentrant child notifications never create a second mutable App reference.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if message == WM_DESTROY {
            PostQuitMessage(0);
            return 0;
        }
        let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<App>;
        if data.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let Ok(mut app) = (*data).try_borrow_mut() else {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        };
        // Creation messages precede CreateWindowExW returning to run.
        app.hwnd = hwnd;
        match message {
            WM_PAINT => {
                app.paint();
                0
            }
            WM_ERASEBKGND => 1,
            WM_SIZE => {
                app.layout();
                0
            }
            WM_EXITSIZEMOVE => {
                let selected = app.selected;
                app.load(selected);
                0
            }
            WM_COMMAND => {
                app.command((wparam & 0xffff) as u16, ((wparam >> 16) & 0xffff) as u16);
                0
            }
            RESULT => {
                if let Ok(event) = app.events.try_recv() {
                    app.consume(event);
                }
                0
            }
            WM_TIMER => {
                if wparam == ANIMATION {
                    KillTimer(hwnd, ANIMATION);
                    app.animation(None);
                } else if wparam == DEADLINE {
                    if app.smoke == Smoke::Window {
                        app.success = true;
                    }
                    app.close();
                } else if wparam == SHUTDOWN
                    && (app.backend.is_closed()
                        || app
                            .closing
                            .is_some_and(|at| at.elapsed() > Duration::from_millis(1500)))
                {
                    KillTimer(hwnd, SHUTDOWN);
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_CLOSE => {
                app.close();
                0
            }
            WM_LBUTTONDOWN => {
                app.controls_input = false;
                SetFocus(hwnd);
                if app.reader.zoom > 1.0 {
                    app.drag = Some((
                        (lparam as u16) as i16 as i32,
                        ((lparam >> 16) as u16) as i16 as i32,
                    ));
                    SetCapture(hwnd);
                }
                0
            }
            WM_LBUTTONUP => {
                app.drag = None;
                ReleaseCapture();
                0
            }
            WM_MOUSEMOVE => {
                if let Some((x, y)) = app.drag {
                    let point = (
                        (lparam as u16) as i16 as i32,
                        ((lparam >> 16) as u16) as i16 as i32,
                    );
                    app.pan.0 += point.0 - x;
                    app.pan.1 += point.1 - y;
                    app.drag = Some(point);
                    InvalidateRect(hwnd, ptr::null(), 0);
                }
                0
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam >> 16) as u16) as i16 as f64;
                if GetKeyState(VK_CONTROL as i32) < 0 {
                    app.zoom(if delta > 0.0 { 1.1 } else { 1.0 / 1.1 });
                } else if app.reader.zoom > 1.0 {
                    app.pan.1 += (delta / 2.0) as i32;
                    InvalidateRect(hwnd, ptr::null(), 0);
                } else {
                    if app.wheel.signum() != delta.signum() {
                        app.wheel = delta;
                    } else {
                        app.wheel += delta;
                    }
                    if app.wheel.abs() >= 120.0
                        && app
                            .last_wheel
                            .is_none_or(|at| at.elapsed() >= Duration::from_millis(100))
                    {
                        let forward = app.wheel < 0.0;
                        app.wheel = 0.0;
                        app.last_wheel = Some(Instant::now());
                        app.navigate(forward);
                    }
                }
                0
            }
            WM_DROPFILES => {
                let drop = wparam as windows_sys::Win32::UI::Shell::HDROP;
                let count = DragQueryFileW(drop, u32::MAX, ptr::null_mut(), 0);
                let path = if count == 1 {
                    let len = DragQueryFileW(drop, 0, ptr::null_mut(), 0) as usize;
                    if len < ncat_protocol::MAX_PATH_BYTES / 2 {
                        let mut buffer = vec![0u16; len + 1];
                        DragQueryFileW(drop, 0, buffer.as_mut_ptr(), buffer.len() as u32);
                        use std::os::windows::ffi::OsStringExt;
                        Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])))
                    } else {
                        None
                    }
                } else {
                    None
                };
                DragFinish(drop);
                if let Some(path) = path {
                    app.open(path);
                } else {
                    app.text(STATUS, "파일 또는 폴더를 한 개씩 드롭하세요");
                }
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

pub fn run(initial: Option<PathBuf>, smoke: Smoke) -> io::Result<()> {
    let (app, sender) = App::new(smoke)?;
    let state = Box::new(RefCell::new(app));
    let class_name = wide("nCatImage.NativeViewer");
    let title = wide(concat!(
        "nCatImage v",
        env!("CARGO_PKG_VERSION"),
        " [UNTESTED Windows]"
    ));
    // SAFETY: static callback and run-owned state; synchronous calls use live,
    // NUL-terminated buffers. Shared bridges pass HWND integers, never App pointers.
    unsafe {
        let com = CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let instance = GetModuleHandleW(ptr::null());
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&class) == 0 {
            if com >= 0 {
                CoUninitialize();
            }
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1040,
                820,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                (&*state as *const RefCell<App>).cast(),
            );
            if hwnd.is_null() {
                return Err(io::Error::last_os_error());
            }
            let _window = Window(hwnd);
            {
                let mut app = state.borrow_mut();
                app.hwnd = hwnd;
                let specs = [
                    (OPEN, "BUTTON", "파일", 8, 4, 52),
                    (FOLDER, "BUTTON", "폴더", 64, 4, 52),
                    (ADDRESS, "EDIT", "", 120, 4, 265),
                    (CONNECT, "BUTTON", "주소 열기", 389, 4, 80),
                    (PREVIOUS, "BUTTON", "이전", 473, 4, 52),
                    (PAGE, "EDIT", "1", 529, 4, 50),
                    (PAGE_GO, "BUTTON", "이동", 583, 4, 48),
                    (NEXT, "BUTTON", "다음", 635, 4, 52),
                    (DIRECTION, "BUTTON", "좌→우", 691, 4, 62),
                    (SMALLER, "BUTTON", "−", 757, 4, 32),
                    (FIT, "BUTTON", "맞춤", 793, 4, 48),
                    (LARGER, "BUTTON", "+", 845, 4, 32),
                    (ROTATE, "BUTTON", "회전", 881, 4, 48),
                    (FULLSCREEN, "BUTTON", "전체 화면", 933, 4, 90),
                    (FRAME_INFO, "STATIC", "애니메이션 없음", 8, 38, 205),
                    (FRAME_PREVIOUS, "BUTTON", "이전 프레임 ,", 217, 36, 110),
                    (FRAME_NEXT, "BUTTON", "다음 프레임 .", 331, 36, 110),
                    (PLAY, "BUTTON", "일시정지 /", 445, 36, 104),
                    (SPEED, "COMBOBOX", "", 553, 36, 90),
                    (FRAME_NUMBER, "EDIT", "1", 647, 36, 70),
                    (FRAME_GO, "BUTTON", "프레임 이동", 721, 36, 100),
                    (
                        STATUS,
                        "STATIC",
                        "파일/폴더/ZIP/CBZ를 열거나 드롭하세요",
                        8,
                        72,
                        1010,
                    ),
                ];
                for (id, kind, text, x, y, width) in specs {
                    let style = WS_CHILD
                        | WS_VISIBLE
                        | if kind == "STATIC" { 0 } else { WS_TABSTOP }
                        | if kind == "EDIT" {
                            WS_BORDER | ES_AUTOHSCROLL as u32
                        } else if kind == "COMBOBOX" {
                            CBS_DROPDOWNLIST as u32 | WS_VSCROLL
                        } else {
                            0
                        };
                    let control = CreateWindowExW(
                        0,
                        wide(kind).as_ptr(),
                        wide(text).as_ptr(),
                        style,
                        x,
                        y,
                        width,
                        if kind == "COMBOBOX" { 280 } else { 28 },
                        hwnd,
                        id as usize as _,
                        instance,
                        ptr::null(),
                    );
                    if control.is_null() {
                        DestroyWindow(hwnd);
                        return Err(io::Error::last_os_error());
                    }
                    SendMessageW(
                        control,
                        WM_SETFONT,
                        GetStockObject(DEFAULT_GUI_FONT) as usize,
                        1,
                    );
                    app.controls.push((id, control, x, y, width));
                }
                for speed in SPEEDS {
                    SendMessageW(
                        app.control(SPEED),
                        CB_ADDSTRING,
                        0,
                        wide(&format!("{speed}×")).as_ptr() as isize,
                    );
                }
                SendMessageW(app.control(SPEED), CB_SETCURSEL, 3, 0);
                app.playback_ui();
                app.navigation_ui();
                DragAcceptFiles(hwnd, 1);
                app.layout();
            }
            let backend = state.borrow().backend.clone();
            let receiver = backend.events();
            let stop = state.borrow().stop.clone();
            let window = hwnd as usize;
            std::thread::Builder::new()
                .name("ncat-native-events".into())
                .spawn(move || {
                    while let Ok(event) = receiver.recv_blocking() {
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        if event.generation() != backend.generation() {
                            continue;
                        }
                        let mut event = stage(event);
                        loop {
                            if stop.load(Ordering::Acquire) {
                                return;
                            }
                            match sender.try_send(event) {
                                Ok(()) => break,
                                Err(mpsc::TrySendError::Full(back)) => {
                                    event = back;
                                    std::thread::sleep(Duration::from_millis(2));
                                }
                                Err(mpsc::TrySendError::Disconnected(_)) => return,
                            }
                        }
                        if PostMessageW(window as HWND, RESULT, 0, 0) == 0 {
                            break;
                        }
                    }
                })?;
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
            if smoke != Smoke::None {
                SetTimer(
                    hwnd,
                    DEADLINE,
                    if smoke == Smoke::Window {
                        350
                    } else if smoke == Smoke::Navigation {
                        60000
                    } else {
                        20000
                    },
                    None,
                );
            }
            if let Some(path) = initial {
                state.borrow_mut().open(path);
            }
            let mut message: MSG = std::mem::zeroed();
            loop {
                let status = GetMessageW(&mut message, ptr::null_mut(), 0, 0);
                if status < 0 {
                    return Err(io::Error::last_os_error());
                }
                if status == 0 {
                    break;
                }
                if message.message == WM_LBUTTONDOWN && message.hwnd != hwnd {
                    state.borrow_mut().controls_input = true;
                }
                if message.message == WM_KEYDOWN && state.borrow_mut().key(message.wParam as u32) {
                    continue;
                }
                let controls_input = state.borrow().controls_input;
                if controls_input && IsDialogMessageW(hwnd, &message) != 0 {
                    continue;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            state.borrow().backend.shutdown();
            if smoke != Smoke::None && !state.borrow().success {
                return Err(io::Error::other("native GUI smoke failed"));
            }
            if smoke != Smoke::None {
                println!("native_gui_smoke=ok frames={}", state.borrow().smoke_frames);
            }
            Ok(())
        })();
        state.borrow().stop.store(true, Ordering::Release);
        state.borrow().backend.shutdown();
        UnregisterClassW(class_name.as_ptr(), instance);
        if com >= 0 {
            CoUninitialize();
        }
        result
    }
}

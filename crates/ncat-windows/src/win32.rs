//! Win32 boundary: HWNDs stay on the UI thread, callbacks borrow through RefCell
//! to tolerate synchronous control notifications. Pixel staging runs off-thread.
use ncat_backend::{Backend, BackendOptions, Book, Event, Frame, View, WorkerCommand};
use ncat_presentation::{
    ReaderView, adjacent,
    input::{self, Command, Key, Modifiers},
    playback::{Playback, SPEEDS},
    reader_bars::{Bar, ReaderBars},
};
use std::{
    cell::{Cell, RefCell},
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
    Foundation::{
        ERROR_NOT_ENOUGH_QUOTA, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
    },
    Graphics::Gdi::*,
    System::{Com::*, LibraryLoader::GetModuleHandleW},
    UI::{
        Controls::{Dialogs::*, WM_MOUSELEAVE},
        Input::KeyboardAndMouse::*,
        Shell::*,
        WindowsAndMessaging::*,
    },
};

const RESULT: u32 = WM_APP + 1;
const VIEWPORT_CHANGED: u32 = WM_APP + 2;
const ANIMATION: usize = 1;
const SHUTDOWN: usize = 2;
const DEADLINE: usize = 3;
const RESIZE: usize = 4;
const RESIZE_TEST: usize = 5;
const METADATA_TEST: usize = 6;
const VIEW_SETTLE_MS: u32 = 100;
const TOP_SIZE: (i32, i32) = (622, 44);
const BOTTOM_SIZE: (i32, i32) = (676, 104);
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
const TOP_PIN: u16 = 123;
const BOTTOM_PIN: u16 = 124;

fn control_bar(id: u16) -> Bar {
    match id {
        OPEN | FOLDER | ADDRESS | CONNECT | DIRECTION | FULLSCREEN | TOP_PIN => Bar::Top,
        _ => Bar::Bottom,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Smoke {
    None,
    Window,
    Resize,
    ResizeFrame,
    MetadataWindow,
    MetadataFrame,
    Frame,
    Animation,
    Navigation,
    ModalLoad,
}

struct Prepared {
    event: Event,
    pixels: Option<Vec<u8>>,
}
struct Display {
    frame: Arc<Frame>,
    bgra: Vec<u8>,
}

// One UI-thread-owned surface. Round up to reuse it across nearby sizes;
// cap the extra 32-bpp storage at 32 MiB, with a no-buffer fallback.
struct PaintBuffer {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    width: i32,
    height: i32,
}
impl PaintBuffer {
    unsafe fn ensure(slot: &mut Option<Self>, dc: HDC, width: i32, height: i32) {
        if slot
            .as_ref()
            .is_some_and(|b| b.width >= width && b.height >= height)
        {
            return;
        }
        *slot = None;
        let round = |n: i32| ((n as i64 + 127) / 128) * 128;
        let (width, height) = (round(width), round(height));
        if width * height > 32 * 1024 * 1024 / 4 {
            return;
        }
        let (width, height) = (width as i32, height as i32);
        // SAFETY: the bitmap and memory DC are paired on the UI thread; the
        // DIB bits are never accessed outside GDI and the bitmap is deselected on drop.
        unsafe {
            let memory = CreateCompatibleDC(dc);
            if memory.is_null() {
                return;
            }
            let mut bits = ptr::null_mut();
            let bitmap = CreateDIBSection(
                dc,
                &bitmap_info(width, height),
                DIB_RGB_COLORS,
                &mut bits,
                ptr::null_mut(),
                0,
            );
            if !bitmap.is_null() {
                let previous = SelectObject(memory, bitmap);
                if !previous.is_null() {
                    *slot = Some(Self {
                        dc: memory,
                        bitmap,
                        previous,
                        width,
                        height,
                    });
                    return;
                }
                DeleteObject(bitmap);
            }
            DeleteDC(memory);
        }
    }
}
impl Drop for PaintBuffer {
    fn drop(&mut self) {
        // SAFETY: both handles belong to this buffer; restore the stock bitmap first.
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

fn bitmap_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    }
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
    metadata: ncat_presentation::metadata::MetadataOverlay,
    metadata_hwnd: HWND,
    metadata_bounds: Cell<Option<(i32, i32, i32)>>,
    hwnd: HWND,
    controls: Vec<(u16, HWND, i32, i32)>,
    paint_buffer: Option<PaintBuffer>,
    bar_tint: Option<PaintBuffer>,
    bars: ReaderBars,
    visible_bars: [bool; 2],
    bar_layout_size: Option<(i32, i32)>,
    live_sizing: bool,
    tracking_mouse: HWND,
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
    modal_load_generation: Option<u64>,
    success: bool,
    resized_view: Option<View>,
    view_deadline: Option<Instant>,
    resize_test_phase: u8,
    resize_test_expected: Option<(View, u32)>,
    paint_count: u32,
    buffered_picture: Option<(i32, i32)>,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

unsafe fn set_child_visible(hwnd: HWND, show: bool) {
    // SAFETY: caller owns the child on this thread. Check its own style, not
    // ancestor visibility (the parent can still be hidden during creation).
    unsafe {
        if (GetWindowLongPtrW(hwnd, GWL_STYLE) & WS_VISIBLE as isize != 0) != show {
            ShowWindow(hwnd, if show { SW_SHOWNA } else { SW_HIDE });
        }
    }
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
                metadata: ncat_presentation::metadata::MetadataOverlay::default(),
                metadata_hwnd: ptr::null_mut(),
                metadata_bounds: Cell::new(None),
                hwnd: ptr::null_mut(),
                controls: Vec::new(),
                paint_buffer: None,
                bar_tint: None,
                bars: ReaderBars::default(),
                visible_bars: [false; 2],
                bar_layout_size: None,
                live_sizing: false,
                tracking_mouse: ptr::null_mut(),
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
                modal_load_generation: None,
                success: false,
                resized_view: None,
                view_deadline: None,
                resize_test_phase: 0,
                resize_test_expected: None,
                paint_count: 0,
                buffered_picture: None,
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
    fn bar_rect(bar: Bar, viewport: RECT) -> RECT {
        let ((width, height), top) = match bar {
            Bar::Top => (TOP_SIZE, 8),
            Bar::Bottom => (BOTTOM_SIZE, viewport.bottom - BOTTOM_SIZE.1 - 8),
        };
        let left = ((viewport.right - width) / 2).max(0);
        RECT {
            left,
            right: left + width,
            top,
            bottom: top + height,
        }
    }

    unsafe fn layout(&mut self) {
        unsafe {
            let viewport = self.viewport();
            if viewport.right == 0 || viewport.bottom == 0 || IsIconic(self.hwnd) != 0 {
                self.bar_layout_size = None;
                return;
            }
            let size = (viewport.right, viewport.bottom);
            let moved = self.bar_layout_size != Some(size);
            self.bar_layout_size = Some(size);
            let focus = GetFocus();
            let editing = [ADDRESS, PAGE, FRAME_NUMBER, SPEED]
                .into_iter()
                .find(|&id| !focus.is_null() && focus == self.control(id))
                .map(control_bar);
            let next = [Bar::Top, Bar::Bottom].map(|bar| self.bars.visible(bar, editing));
            let changed = next != self.visible_bars;
            if moved || changed {
                let mut batch = BeginDeferWindowPos(self.controls.len() as i32);
                for &(id, hwnd, x, y) in &self.controls {
                    let bar = control_bar(id);
                    if !batch.is_null() {
                        let rect = Self::bar_rect(bar, viewport);
                        batch = DeferWindowPos(
                            batch,
                            hwnd,
                            ptr::null_mut(),
                            rect.left + x,
                            rect.top + y,
                            0,
                            0,
                            Self::bar_position_flags(moved, changed, next[bar as usize]),
                        );
                    }
                }
                if batch.is_null() || EndDeferWindowPos(batch) == 0 {
                    for &(id, hwnd, x, y) in &self.controls {
                        let bar = control_bar(id);
                        let rect = Self::bar_rect(bar, viewport);
                        SetWindowPos(
                            hwnd,
                            ptr::null_mut(),
                            rect.left + x,
                            rect.top + y,
                            0,
                            0,
                            Self::bar_position_flags(moved, changed, next[bar as usize]),
                        );
                    }
                }
                self.visible_bars = next;
                self.metadata_layout();
                // Do not copy individual controls over the old image/bar while
                // resizing. Repaint the parent background and children together.
                RedrawWindow(
                    self.hwnd,
                    ptr::null(),
                    ptr::null_mut(),
                    RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
                );
            }
        }
    }

    fn bar_position_flags(moved: bool, changed: bool, visible: bool) -> u32 {
        SWP_NOSIZE
            | SWP_NOZORDER
            | SWP_NOACTIVATE
            | SWP_NOREDRAW
            | SWP_NOCOPYBITS
            | if moved { 0 } else { SWP_NOMOVE }
            | if !changed {
                0
            } else if visible {
                SWP_SHOWWINDOW
            } else {
                SWP_HIDEWINDOW
            }
    }

    unsafe fn sync_pins(&mut self) {
        unsafe {
            for (bar, id) in [(Bar::Top, TOP_PIN), (Bar::Bottom, BOTTOM_PIN)] {
                self.text(
                    id,
                    if self.bars.pinned(bar) {
                        "고정 해제"
                    } else {
                        "고정"
                    },
                );
            }
            self.layout();
        }
    }

    unsafe fn track_bars(&mut self, target: HWND, leaving: bool) {
        unsafe {
            if leaving && self.tracking_mouse == target {
                self.tracking_mouse = ptr::null_mut();
            }
            if !leaving && self.tracking_mouse != target {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: target,
                    dwHoverTime: 0,
                };
                if TrackMouseEvent(&mut track) != 0 {
                    self.tracking_mouse = target;
                }
            }
            let viewport = self.viewport();
            let mut point = POINT::default();
            let inside = GetCursorPos(&mut point) != 0
                && ScreenToClient(self.hwnd, &mut point) != 0
                && (0..viewport.right).contains(&point.x);
            let before = self.bars;
            self.bars.point(
                inside.then_some(point.y),
                viewport.bottom,
                14,
                TOP_SIZE.1 + 10,
                BOTTOM_SIZE.1 + 10,
            );
            if self.bars != before {
                self.layout();
            }
        }
    }
    unsafe fn metadata_layout(&self) {
        unsafe {
            if self.metadata_hwnd.is_null() {
                return;
            }
            let viewport = self.viewport();
            let width = (viewport.right - 24).min(480);
            let columns = ((width - 24) / 8).max(1) as usize;
            let lines: usize = self
                .metadata
                .text
                .lines()
                .map(|line| line.chars().count().div_ceil(columns).max(1))
                .sum();
            let top = if self.visible_bars[Bar::Top as usize] {
                TOP_SIZE.1 + 18
            } else {
                12
            };
            let bottom = if self.visible_bars[Bar::Bottom as usize] {
                BOTTOM_SIZE.1 + 12
            } else {
                12
            };
            let height = (viewport.bottom - top - bottom)
                .min(300)
                .min((lines * 20 + 20) as i32);
            let show = self.metadata.visible() && width >= 80 && height >= 40;
            let bounds = (top, width, height);
            if show && self.metadata_bounds.get() != Some(bounds) {
                SetWindowPos(
                    self.metadata_hwnd,
                    ptr::null_mut(),
                    12,
                    bounds.0,
                    width,
                    height,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                self.metadata_bounds.set(Some(bounds));
            }
            set_child_visible(self.metadata_hwnd, show);
        }
    }
    unsafe fn sync_metadata(&self) {
        unsafe {
            if !self.metadata_hwnd.is_null() {
                SetWindowTextW(
                    self.metadata_hwnd,
                    wide(&self.metadata.text.replace('\n', "\r\n")).as_ptr(),
                );
            }
            self.metadata_layout();
            self.invalidate_picture();
        }
    }
    unsafe fn metadata_test(&mut self) {
        unsafe {
            KillTimer(self.hwnd, METADATA_TEST);
            let result = (|| {
                if self.metadata.enabled || IsWindowVisible(self.metadata_hwnd) != 0 {
                    return false;
                }
                if self.smoke == Smoke::MetadataWindow {
                    // UI-only fixture for checks that cannot start a protected worker.
                    self.metadata.update(&[ncat_protocol::MetadataEntry {
                        name: "Title".into(),
                        value: "<literal> 한글 synthetic metadata".into(),
                    }]);
                } else if !self.metadata.text.contains("<literal>") {
                    return false;
                }
                self.sync_metadata();
                if IsWindowVisible(self.metadata_hwnd) != 0 {
                    return false;
                }
                let focus = GetFocus();
                if !self.key(VK_TAB as u32) || IsWindowVisible(self.metadata_hwnd) == 0 {
                    return false;
                }
                if GetFocus() != focus
                    || self.metadata_bounds.get().is_none_or(|(top, _, _)| {
                        top < Self::bar_rect(Bar::Top, self.viewport()).bottom
                    })
                {
                    return false;
                }
                if !self.key(VK_TAB as u32) || IsWindowVisible(self.metadata_hwnd) != 0 {
                    return false;
                }
                self.key(VK_TAB as u32);
                self.metadata.clear();
                self.sync_metadata();
                IsWindowVisible(self.metadata_hwnd) == 0
            })();
            self.success = result;
            if result {
                println!(
                    "metadata_window_smoke=ok synthetic={} default_off=true tab=true focus_unchanged=true",
                    self.smoke == Smoke::MetadataWindow,
                );
            }
            self.close();
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
    unsafe fn queue_resize(&self) {
        unsafe {
            InvalidateRect(self.hwnd, ptr::null(), 0);
            // SetWindowPos/ShowWindow can synchronously reenter procedure while
            // App is borrowed. Defer state changes until that borrow has ended.
            PostMessageW(self.hwnd, VIEWPORT_CHANGED, 0, 0);
        }
    }
    unsafe fn invalidate_picture(&self) {
        unsafe {
            InvalidateRect(self.hwnd, &self.viewport(), 0);
        }
    }
    unsafe fn viewport_changed(&mut self) {
        unsafe {
            self.layout();
            self.metadata_layout();
            self.queue_view_refresh();
        }
    }
    unsafe fn queue_view_refresh(&mut self) {
        unsafe {
            self.cancel_view_refresh();
            if self.closing.is_some() || IsIconic(self.hwnd) != 0 {
                return;
            }
            let rect = self.viewport();
            if rect.right <= 0 || rect.bottom <= rect.top {
                return;
            }
            let view = self.view();
            self.resized_view = Some(view);
            if view != self.animation_view && self.book.as_ref().is_some_and(|book| book.complete) {
                self.view_deadline =
                    Some(Instant::now() + Duration::from_millis(u64::from(VIEW_SETTLE_MS)));
                SetTimer(self.hwnd, RESIZE, VIEW_SETTLE_MS, None);
            }
        }
    }
    unsafe fn cancel_view_refresh(&mut self) {
        unsafe {
            KillTimer(self.hwnd, RESIZE);
            self.view_deadline = None;
        }
    }
    unsafe fn resize_finished(&mut self) {
        unsafe {
            // KillTimer cannot remove an already posted WM_TIMER.
            if self.view_deadline.is_none_or(|at| Instant::now() < at) {
                return;
            }
            self.cancel_view_refresh();
            if self.closing.is_none()
                && IsIconic(self.hwnd) == 0
                && self.view() != self.animation_view
            {
                self.load(self.selected);
            }
        }
    }
    unsafe fn resize_test(&mut self) {
        unsafe {
            if let Some((expected, paints)) = self.resize_test_expected.take() {
                // The window manager may adjust maximize bounds asynchronously.
                // Validate the settled client area, not the immediate ShowWindow result.
                if self.resized_view != Some(self.view())
                    || self.paint_count <= paints
                    || !self.bar_geometry_matches()
                    || (self.smoke == Smoke::ResizeFrame
                        && self.buffered_picture != Some(self.picture_dimensions(self.viewport())))
                {
                    eprintln!(
                        "nCatImage: resize phase={} expected={expected:?} actual={:?} paints={paints}->{} buffered={:?}",
                        self.resize_test_phase,
                        self.resized_view,
                        self.paint_count,
                        self.buffered_picture,
                    );
                    self.close();
                    return;
                }
            }
            match self.resize_test_phase {
                0 => {
                    SetWindowPos(
                        self.hwnd,
                        ptr::null_mut(),
                        0,
                        0,
                        720,
                        540,
                        SWP_NOMOVE | SWP_NOZORDER,
                    );
                }
                1 => {
                    ShowWindow(self.hwnd, SW_MAXIMIZE);
                }
                2 => {
                    ShowWindow(self.hwnd, SW_RESTORE);
                }
                3 | 4 => self.fullscreen(),
                5 | 6 => {
                    let view = self.view();
                    let generation = self.backend.generation();
                    if self.resize_test_phase == 5 {
                        self.bars.toggle_both();
                        self.bars.point(None, 0, 0, 0, 0);
                    } else {
                        self.bars.toggle_pin(Bar::Top);
                        let height = self.viewport().bottom;
                        self.bars
                            .point(Some(height - 2), height, 14, TOP_SIZE.1, BOTTOM_SIZE.1);
                    }
                    self.sync_pins();
                    let expected = if self.resize_test_phase == 5 {
                        [false, false]
                    } else {
                        [true, true]
                    };
                    if self.visible_bars != expected
                        || self.view() != view
                        || self.backend.generation() != generation
                    {
                        eprintln!("nCatImage: overlay visibility changed the image view or decode");
                        self.close();
                        return;
                    }
                }
                7 => {
                    ShowWindow(self.hwnd, SW_MINIMIZE);
                }
                8 => {
                    ShowWindow(self.hwnd, SW_RESTORE);
                }
                9 => {
                    for (width, height) in [(780, 470), (940, 700), (720, 300), (860, 570)] {
                        SetWindowPos(
                            self.hwnd,
                            ptr::null_mut(),
                            0,
                            0,
                            width,
                            height,
                            SWP_NOMOVE | SWP_NOZORDER,
                        );
                    }
                }
                10 | 11 if self.smoke == Smoke::ResizeFrame => {
                    let generation = self.backend.generation();
                    if self.resize_test_phase == 10 {
                        for _ in 0..4 {
                            self.zoom(1.25);
                        }
                    } else {
                        self.zoom(0.0);
                    }
                    if self.frame.is_none()
                        || self.backend.generation() != generation
                        || self.view_deadline.is_none()
                    {
                        eprintln!("nCatImage: zoom must retain the frame and defer decoding");
                        self.close();
                        return;
                    }
                }
                _ => {
                    self.success = true;
                    self.close();
                    return;
                }
            }
            self.resize_test_phase += 1;
            if IsIconic(self.hwnd) == 0 {
                self.resize_test_expected = Some((self.view(), self.paint_count));
            }
        }
    }
    unsafe fn bar_geometry_matches(&self) -> bool {
        unsafe {
            let viewport = self.viewport();
            let mut origin = POINT::default();
            if ClientToScreen(self.hwnd, &mut origin) == 0 {
                return false;
            }
            self.controls.iter().all(|&(id, hwnd, x, y)| {
                let bar = control_bar(id);
                let expected = Self::bar_rect(bar, viewport);
                let mut actual = RECT::default();
                let visible = IsWindowVisible(hwnd) != 0;
                let valid = GetWindowRect(hwnd, &mut actual) != 0
                    && actual.left - origin.x == expected.left + x
                    && actual.top - origin.y == expected.top + y
                    && visible == self.visible_bars[bar as usize];
                if !valid {
                    eprintln!(
                        "bar control {id}: actual=({}, {}, {visible}) expected=({}, {}, {})",
                        actual.left - origin.x,
                        actual.top - origin.y,
                        expected.left + x,
                        expected.top + y,
                        self.visible_bars[bar as usize]
                    );
                }
                valid
            })
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
            self.cancel_view_refresh();
            self.reset(true);
            self.book = None;
            self.frame = None;
            self.metadata.clear();
            self.sync_metadata();
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
            self.cancel_view_refresh();
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
            let previous = self.reader.zoom;
            self.reader.change_zoom(factor);
            if self.reader.zoom == previous && self.pan == (0, 0) {
                return;
            }
            self.pan = (0, 0);
            self.invalidate_picture();
            self.queue_view_refresh();
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
            self.queue_resize();
        }
    }
    unsafe fn consume(&mut self, prepared: Prepared) {
        unsafe {
            if self.closing.is_some() || prepared.event.generation() != self.backend.generation() {
                return;
            }
            let generation = prepared.event.generation();
            match prepared.event {
                Event::Catalog { book, .. } => {
                    self.selected = book.selected;
                    self.book = Some(book);
                    self.navigation_ui();
                    // A resize may have arrived while the initial catalog was
                    // incomplete and could not yet submit a replacement load.
                    self.viewport_changed();
                }
                Event::Frame { frame, .. } => {
                    let Some(pixels) = prepared.pixels else {
                        return;
                    };
                    if self.metadata.update(&frame.info.metadata) {
                        self.sync_metadata();
                    }
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
                    let first_frame = self.frame.is_none();
                    self.frame = Some(Display {
                        frame,
                        bgra: pixels,
                    });
                    self.playback_ui();
                    self.schedule();
                    self.invalidate_picture();
                    self.smoke_frames += 1;
                    if self.smoke == Smoke::ResizeFrame && first_frame {
                        SetTimer(self.hwnd, RESIZE_TEST, 250, None);
                    }
                    if self.smoke == Smoke::MetadataFrame && first_frame {
                        self.metadata_test();
                    }
                    if self.modal_load_generation == Some(generation) {
                        self.success = self.reader.original == (1200, 800);
                        self.close();
                    }
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
                    self.metadata.clear();
                    self.sync_metadata();
                    self.text(STATUS, &code.to_string());
                    InvalidateRect(self.hwnd, ptr::null(), 0);
                    if self.smoke != Smoke::None {
                        eprintln!("nCatImage: GUI smoke error={code:?}");
                        self.close();
                    }
                }
            }
            if self.smoke == Smoke::ModalLoad
                && self.modal_load_generation.is_none()
                && self.frame.is_some()
                && self.book.as_ref().is_some_and(|book| book.complete)
            {
                self.load(2);
                self.modal_load_generation = Some(self.backend.generation());
                let mut message = MSG::default();
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    if PeekMessageW(&mut message, self.hwnd, RESULT, RESULT, PM_REMOVE) != 0 {
                        DispatchMessageW(&message);
                        eprintln!("nested pump dispatched page result while App was borrowed");
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                eprintln!("nested pump did not receive page result");
                self.close();
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
            KillTimer(self.hwnd, RESIZE);
            KillTimer(self.hwnd, RESIZE_TEST);
            KillTimer(self.hwnd, METADATA_TEST);
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
                TOP_PIN | BOTTOM_PIN => {
                    self.bars.toggle_pin(control_bar(id));
                    self.sync_pins();
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
                Command::ToggleMetadata => {
                    self.metadata.toggle();
                    self.sync_metadata();
                }
                Command::Open { folder } => self.command(if folder { FOLDER } else { OPEN }, 0),
                Command::Network => {
                    if self.fullscreen.is_some() {
                        self.fullscreen();
                    }
                    self.controls_input = true;
                    if !self.visible_bars[Bar::Top as usize] {
                        self.bars.point(
                            Some(0),
                            self.viewport().bottom,
                            14,
                            TOP_SIZE.1,
                            BOTTOM_SIZE.1,
                        );
                        self.layout();
                    }
                    SetFocus(self.control(ADDRESS));
                }
                Command::Quit => self.close(),
                Command::Fullscreen => self.fullscreen(),
                Command::ToggleInterface => {
                    SetFocus(self.hwnd);
                    self.controls_input = false;
                    self.bars.toggle_both();
                    self.sync_pins();
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
    fn picture_dimensions(&self, rect: RECT) -> (i32, i32) {
        let height = rect.bottom - rect.top;
        self.reader
            .picture_size((rect.right, height))
            .unwrap_or_else(|| {
                let (w, h) = ncat_core::fit_size(
                    self.reader.original.0,
                    self.reader.original.1,
                    rect.right.max(1) as u32,
                    height.max(1) as u32,
                )
                .unwrap_or((1, 1));
                (w as i32, h as i32)
            })
    }

    unsafe fn draw_picture(&mut self, dc: HDC, rect: RECT) -> Option<(i32, i32)> {
        let (w, h) = self.picture_dimensions(rect);
        let height = rect.bottom - rect.top;
        let mut drawn = None;
        // SAFETY: dc is a live paint or memory DC, clipped to the picture area.
        // Display owns the validated, tightly packed top-down BGRA pixels.
        unsafe {
            let brush = GetStockObject(DC_BRUSH);
            SetDCBrushColor(dc, 0x202020);
            if let Some(display) = &self.frame {
                let max_x = (w - rect.right).max(0) / 2;
                let max_y = (h - height).max(0) / 2;
                self.pan.0 = self.pan.0.clamp(-max_x, max_x);
                self.pan.1 = self.pan.1.clamp(-max_y, max_y);
                let x = (rect.right - w) / 2 + self.pan.0;
                let y = rect.top + (height - h) / 2 + self.pan.1;
                let width = display.frame.info.width as i32;
                let height = display.frame.info.height as i32;
                SetStretchBltMode(dc, HALFTONE);
                SetBrushOrgEx(dc, 0, 0, ptr::null_mut());
                let lines = StretchDIBits(
                    dc,
                    x,
                    y,
                    w,
                    h,
                    0,
                    0,
                    width,
                    height,
                    display.bgra.as_ptr().cast(),
                    &bitmap_info(width, height),
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                if lines > 0 {
                    // The fallback draws directly to the window. Fill only the
                    // gutters so it never erases the image before replacing it.
                    let left = x.clamp(0, rect.right);
                    let right = (x + w).clamp(0, rect.right);
                    let top = y.clamp(rect.top, rect.bottom);
                    let bottom = (y + h).clamp(rect.top, rect.bottom);
                    for (left, top, right, bottom) in [
                        (0, rect.top, rect.right, top),
                        (0, bottom, rect.right, rect.bottom),
                        (0, top, left, bottom),
                        (right, top, rect.right, bottom),
                    ] {
                        if left < right && top < bottom {
                            FillRect(
                                dc,
                                &RECT {
                                    left,
                                    top,
                                    right,
                                    bottom,
                                },
                                brush,
                            );
                        }
                    }
                    drawn = Some((w, h));
                }
            }
            if drawn.is_none() {
                FillRect(dc, &rect, brush);
            }
        }
        drawn
    }

    unsafe fn paint(&mut self) {
        unsafe {
            let mut paint: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(self.hwnd, &mut paint);
            if !dc.is_null() {
                self.paint_count = self.paint_count.saturating_add(1);
                let rect = self.viewport();
                let mut dirty = RECT::default();
                if IntersectRect(&mut dirty, &rect, &paint.rcPaint) != 0 {
                    PaintBuffer::ensure(&mut self.paint_buffer, dc, rect.right, rect.bottom);
                    self.buffered_picture = None;
                    if let Some(memory) = self.paint_buffer.as_ref().map(|b| b.dc) {
                        IntersectClipRect(memory, dirty.left, dirty.top, dirty.right, dirty.bottom);
                        let drawn = self.draw_picture(memory, rect);
                        self.draw_bars(memory, rect);
                        SelectClipRgn(memory, ptr::null_mut());
                        if BitBlt(
                            dc,
                            dirty.left,
                            dirty.top,
                            dirty.right - dirty.left,
                            dirty.bottom - dirty.top,
                            memory,
                            dirty.left,
                            dirty.top,
                            SRCCOPY,
                        ) != 0
                        {
                            self.buffered_picture = drawn;
                        } else {
                            IntersectClipRect(dc, rect.left, rect.top, rect.right, rect.bottom);
                            self.draw_picture(dc, rect);
                            self.draw_bars(dc, rect);
                        }
                    } else {
                        IntersectClipRect(dc, rect.left, rect.top, rect.right, rect.bottom);
                        self.draw_picture(dc, rect);
                        self.draw_bars(dc, rect);
                    }
                }
            }
            EndPaint(self.hwnd, &paint);
        }
    }

    unsafe fn draw_bars(&mut self, dc: HDC, viewport: RECT) {
        // SAFETY: this paint runs on the owning UI thread. Temporary regions
        // and the saved clip are released before returning to the caller.
        unsafe {
            if !self.visible_bars.iter().any(|&visible| visible) {
                return;
            }
            PaintBuffer::ensure(&mut self.bar_tint, dc, 1, 1);
            let brush = GetStockObject(DC_BRUSH);
            SetDCBrushColor(dc, 0x181818);
            if let Some(tint) = &self.bar_tint {
                SetDCBrushColor(tint.dc, 0x181818);
                FillRect(
                    tint.dc,
                    &RECT {
                        right: 1,
                        bottom: 1,
                        ..Default::default()
                    },
                    brush,
                );
            }
            for bar in [Bar::Top, Bar::Bottom] {
                if !self.visible_bars[bar as usize] {
                    continue;
                }
                let r = Self::bar_rect(bar, viewport);
                let region = CreateRoundRectRgn(r.left, r.top, r.right, r.bottom, 20, 20);
                if region.is_null() {
                    continue;
                }
                let saved = SaveDC(dc);
                let blended = if saved != 0 {
                    let ok = ExtSelectClipRgn(dc, region, RGN_AND) != RGN_ERROR
                        && self.bar_tint.as_ref().is_some_and(|tint| {
                            GdiAlphaBlend(
                                dc,
                                r.left,
                                r.top,
                                r.right - r.left,
                                r.bottom - r.top,
                                tint.dc,
                                0,
                                0,
                                1,
                                1,
                                BLENDFUNCTION {
                                    BlendOp: AC_SRC_OVER as u8,
                                    BlendFlags: 0,
                                    SourceConstantAlpha: 225,
                                    AlphaFormat: 0,
                                },
                            ) != 0
                        });
                    RestoreDC(dc, saved);
                    ok
                } else {
                    false
                };
                if !blended {
                    FillRgn(dc, region, brush);
                }
                DeleteObject(region);
            }
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
        if message == WM_ERASEBKGND {
            return 1;
        }
        if message == WM_NCCALCSIZE && wparam != 0 {
            // The image and floating bars are anchored differently. No region
            // from the old client surface is valid to copy when its size changes.
            DefWindowProcW(hwnd, message, wparam, lparam);
            return WVR_REDRAW as LRESULT;
        }
        if message == WM_GETMINMAXINFO {
            let info = &mut *(lparam as *mut MINMAXINFO);
            info.ptMinTrackSize.x = info.ptMinTrackSize.x.max(BOTTOM_SIZE.0 + 32);
            info.ptMinTrackSize.y = info.ptMinTrackSize.y.max(260);
            return 0;
        }
        if message == WM_CTLCOLORSTATIC
            && matches!(GetDlgCtrlID(lparam as HWND) as u16, STATUS | FRAME_INFO)
        {
            let dc = wparam as HDC;
            SetTextColor(dc, 0x00ffffff);
            SetBkColor(dc, 0x181818);
            SetDCBrushColor(dc, 0x181818);
            return GetStockObject(DC_BRUSH) as isize;
        }
        if matches!(message, WM_SIZE | WM_ENTERSIZEMOVE | WM_EXITSIZEMOVE) {
            // Interactive sizing can repaint before the posted viewport update.
            // Align controls now when possible, but keep decoding deferred.
            let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<App>;
            if !state.is_null()
                && let Ok(mut app) = (*state).try_borrow_mut()
            {
                if message == WM_ENTERSIZEMOVE {
                    app.live_sizing = true;
                } else if message == WM_EXITSIZEMOVE {
                    app.live_sizing = false;
                } else if app.live_sizing && !app.controls.is_empty() {
                    app.layout();
                }
            }
            if message == WM_ENTERSIZEMOVE {
                return 0;
            }
            InvalidateRect(hwnd, ptr::null(), 0);
            PostMessageW(
                hwnd,
                VIEWPORT_CHANGED,
                usize::from(message == WM_SIZE && wparam == SIZE_MINIMIZED as usize),
                0,
            );
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
            WM_CTLCOLORSTATIC if lparam as HWND == app.metadata_hwnd => {
                let dc = wparam as windows_sys::Win32::Graphics::Gdi::HDC;
                SetTextColor(dc, 0x00ffffff);
                SetBkColor(dc, 0);
                GetStockObject(BLACK_BRUSH) as isize
            }
            VIEWPORT_CHANGED => {
                // A synchronous WM_PAINT during a window transition can consume
                // the initial invalidation while App is still borrowed.
                app.invalidate_picture();
                if wparam != 0 {
                    app.bar_layout_size = None;
                }
                app.viewport_changed();
                0
            }
            WM_COMMAND => {
                app.command((wparam & 0xffff) as u16, ((wparam >> 16) & 0xffff) as u16);
                0
            }
            // Wake only. A modal loop can dispatch this while App is borrowed;
            // the outer message loop drains the data after the modal call returns.
            RESULT => 0,
            WM_TIMER => {
                if wparam == RESIZE {
                    app.resize_finished();
                } else if wparam == METADATA_TEST {
                    app.metadata_test();
                } else if wparam == RESIZE_TEST {
                    app.resize_test();
                } else if wparam == ANIMATION {
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
                app.layout();
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
                    app.invalidate_picture();
                }
                0
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam >> 16) as u16) as i16 as f64;
                if GetKeyState(VK_CONTROL as i32) < 0 {
                    app.zoom(if delta > 0.0 { 1.1 } else { 1.0 / 1.1 });
                } else if app.reader.zoom > 1.0 {
                    app.pan.1 += (delta / 2.0) as i32;
                    app.invalidate_picture();
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
                    (OPEN, "BUTTON", "파일", 8, 8, 52),
                    (FOLDER, "BUTTON", "폴더", 64, 8, 52),
                    (ADDRESS, "EDIT", "", 120, 8, 182),
                    (CONNECT, "BUTTON", "주소 열기", 306, 8, 80),
                    (DIRECTION, "BUTTON", "좌→우", 390, 8, 62),
                    (FULLSCREEN, "BUTTON", "전체 화면", 456, 8, 82),
                    (TOP_PIN, "BUTTON", "고정 해제", 542, 8, 72),
                    (PREVIOUS, "BUTTON", "이전", 120, 8, 52),
                    (PAGE, "EDIT", "1", 176, 8, 50),
                    (PAGE_GO, "BUTTON", "이동", 230, 8, 48),
                    (NEXT, "BUTTON", "다음", 282, 8, 52),
                    (SMALLER, "BUTTON", "−", 338, 8, 32),
                    (FIT, "BUTTON", "맞춤", 374, 8, 48),
                    (LARGER, "BUTTON", "+", 426, 8, 32),
                    (ROTATE, "BUTTON", "회전", 462, 8, 48),
                    (BOTTOM_PIN, "BUTTON", "고정 해제", 596, 8, 72),
                    (FRAME_INFO, "STATIC", "애니메이션 없음", 10, 46, 184),
                    (FRAME_PREVIOUS, "BUTTON", "이전 프레임", 198, 41, 83),
                    (FRAME_NEXT, "BUTTON", "다음 프레임", 285, 41, 83),
                    (PLAY, "BUTTON", "일시정지 /", 372, 41, 80),
                    (SPEED, "COMBOBOX", "", 456, 41, 58),
                    (FRAME_NUMBER, "EDIT", "1", 518, 41, 59),
                    (FRAME_GO, "BUTTON", "프레임 이동", 581, 41, 87),
                    (
                        STATUS,
                        "STATIC",
                        "파일/폴더/ZIP/CBZ를 열거나 드롭하세요",
                        10,
                        78,
                        656,
                    ),
                ];
                for (id, kind, text, x, y, width) in specs {
                    let style = WS_CHILD
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
                        if kind == "COMBOBOX" {
                            280
                        } else if kind == "STATIC" {
                            18
                        } else {
                            28
                        },
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
                    app.controls.push((id, control, x, y));
                }
                for speed in SPEEDS {
                    SendMessageW(
                        app.control(SPEED),
                        CB_ADDSTRING,
                        0,
                        wide(&format!("{speed}×")).as_ptr() as isize,
                    );
                }
                app.metadata_hwnd = CreateWindowExW(
                    0,
                    wide("EDIT").as_ptr(),
                    wide("").as_ptr(),
                    WS_CHILD
                        | WS_VSCROLL
                        | WS_BORDER
                        | ES_MULTILINE as u32
                        | ES_READONLY as u32
                        | ES_AUTOVSCROLL as u32,
                    12,
                    116,
                    480,
                    300,
                    hwnd,
                    ptr::null_mut(),
                    instance,
                    ptr::null(),
                );
                if app.metadata_hwnd.is_null() {
                    return Err(io::Error::last_os_error());
                }
                SendMessageW(
                    app.metadata_hwnd,
                    WM_SETFONT,
                    GetStockObject(DEFAULT_GUI_FONT) as usize,
                    1,
                );
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
                        while PostMessageW(window as HWND, RESULT, 0, 0) == 0 {
                            if GetLastError() != ERROR_NOT_ENOUGH_QUOTA
                                || stop.load(Ordering::Acquire)
                            {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(2));
                        }
                    }
                })?;
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
            if smoke == Smoke::Resize {
                SetTimer(hwnd, RESIZE_TEST, 250, None);
            }
            if smoke == Smoke::MetadataWindow {
                SetTimer(hwnd, METADATA_TEST, 300, None);
            }
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
                {
                    let mut app = state.borrow_mut();
                    // Check the queue itself before blocking, even if a nested
                    // message loop already removed the only RESULT wake.
                    while let Ok(event) = app.events.try_recv() {
                        app.consume(event);
                    }
                }
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
                if matches!(message.message, WM_MOUSEMOVE | WM_MOUSELEAVE)
                    && (message.hwnd == hwnd || IsChild(hwnd, message.hwnd) != 0)
                {
                    state
                        .borrow_mut()
                        .track_bars(message.hwnd, message.message == WM_MOUSELEAVE);
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

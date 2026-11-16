#![forbid(unsafe_code)]

mod input;
pub mod network;

use gtk::{gdk, gio, glib, prelude::*};
use ncat_backend::{
    Backend, Book, DropEvent, DropRejection, Event, FrameBytes, NetworkFailure, View, worker_path,
};
use ncat_core::Direction;
use ncat_presentation::playback::Playback;
use ncat_presentation::{ReaderView, adjacent, interface_shown};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

fn single_drop_file(files: &[gio::File]) -> Result<&gio::File, DropRejection> {
    match files {
        [] => Err(DropRejection::Empty),
        [file] => Ok(file),
        _ => Err(DropRejection::MultipleItems),
    }
}

struct Ui {
    window: gtk::ApplicationWindow,
    header: gtk::HeaderBar,
    toolbar: gtk::Box,
    interface_visible: Cell<bool>,
    interface_input: Cell<bool>,
    drop_generation: Cell<Option<u64>>,
    network_request: Cell<u64>,
    network_task: RefCell<Option<glib::JoinHandle<()>>>,
    resolve_network: network::Resolver,
    network_timer: RefCell<Option<glib::SourceId>>,
    picture: gtk::Picture,
    scroll: gtk::ScrolledWindow,
    status: gtk::Label,
    position: gtk::SpinButton,
    previous: gtk::Button,
    next: gtk::Button,
    direction_button: gtk::ToggleButton,
    backend: Backend,
    book: RefCell<Option<Arc<Book>>>,
    selected: Cell<usize>,
    reader_view: Cell<ReaderView>,
    syncing: Cell<bool>,
    closing: Cell<bool>,
    reload_timer: RefCell<Option<glib::SourceId>>,
    loading_timer: RefCell<Option<glib::SourceId>>,
    animation_timer: RefCell<Option<glib::SourceId>>,
    animation_view: Cell<View>,
    playback: Cell<Playback>,
    frame_status: gtk::Label,
    frame_previous: gtk::Button,
    frame_next: gtk::Button,
    frame_target: gtk::SpinButton,
    frame_jump: gtk::Button,
    play_pause: gtk::Button,
    speed: gtk::DropDown,
    smoke: bool,
    smoke_animation: bool,
    smoke_controls: bool,
    smoke_animation_frames: Cell<u32>,
    smoke_sensitivity_changes: Cell<u32>,
    smoke_steps: Cell<Option<u32>>,
    smoke_started: Cell<bool>,
    smoke_ok: Rc<Cell<bool>>,
}

impl Ui {
    fn controls_smoke(self: &Rc<Self>, count: u32) {
        let state = self.playback.get();
        match count {
            1 => {
                self.play_pause.emit_clicked();
                assert!(self.playback.get().paused);
                let weak = Rc::downgrade(self);
                glib::timeout_add_local_once(Duration::from_millis(100), move || {
                    if let Some(ui) = weak.upgrade() {
                        assert_eq!(ui.playback.get().index, 0);
                        assert!(!ui.playback.get().busy);
                        ui.frame_next.emit_clicked();
                    }
                });
            }
            2 => {
                assert!(state.paused);
                assert_eq!(state.index, 1);
                self.frame_previous.emit_clicked();
            }
            3 => {
                assert!(state.paused);
                assert_eq!(state.index, 0);
                self.speed.set_selected(6);
                assert_eq!(self.playback.get().speed, 2.0);
                self.play_pause.emit_clicked();
            }
            4 => {
                assert_eq!(state.index, 1);
                assert!(!state.paused);
                self.play_pause.emit_clicked();
                self.frame_next.emit_clicked();
            }
            5 => {
                assert_eq!(state.index, 0);
                assert!(!state.paused);
                assert!(self.frame_status.text().contains("2.00×"));
                assert!(!self.window.title().unwrap().contains("프레임"));
                assert_eq!(self.frame_status.parent(), self.frame_previous.parent());
                assert!(self.header.title_widget().is_none());
                self.play_pause.emit_clicked();
                assert!(self.frame_jump.is_sensitive());
                self.frame_target.set_value(2.0);
                self.frame_jump.emit_clicked();
            }
            6 => {
                assert!(state.paused);
                assert_eq!(state.index, 1);
                self.frame_target.set_value(1.0);
                self.frame_jump.emit_clicked();
            }
            7 => {
                assert!(state.paused);
                assert_eq!(state.index, 0);
                assert!(self.frame_status.text().contains("1/2"));
                assert_eq!(self.smoke_sensitivity_changes.get(), 0);
                self.cancel_reload();
                self.smoke_ok.set(true);
                println!(
                    "animation_controls_smoke=ok pause=true previous=true next=true speed=2 restart=true toolbar=true jump=true stable_buttons=true"
                );
                self.window.close();
            }
            _ => panic!("unexpected controls smoke frame"),
        }
    }

    fn stop_animation_timer(&self) {
        if let Some(timer) = self.animation_timer.borrow_mut().take() {
            timer.remove();
        }
    }

    fn reset_playback(&self, autoplay: bool) {
        let old = self.playback.get();
        self.playback.set(Playback {
            speed: old.speed,
            paused: !autoplay && old.paused,
            ..Playback::default()
        });
        self.update_playback();
    }

    fn playback_text(&self) -> String {
        let state = self.playback.get();
        if !state.active {
            return String::new();
        }
        format!(
            " · 프레임 {}/{} · {} · {:.2}×",
            state.index + 1,
            state.total.map_or("?".into(), |n| n.to_string()),
            if state.ended {
                "종료"
            } else if state.paused {
                "일시정지"
            } else {
                "재생"
            },
            state.speed
        )
    }

    fn update_playback(&self) {
        let state = self.playback.get();
        let text = self.playback_text();
        let label = if text.is_empty() {
            "애니메이션 없음"
        } else {
            text.trim_start_matches(" · ")
        };
        if self.frame_status.text() != label {
            self.frame_status.set_text(label);
        }
        self.frame_previous
            .set_sensitive(state.frame_controls_enabled());
        self.frame_next
            .set_sensitive(state.frame_controls_enabled());
        self.play_pause.set_sensitive(state.active);
        let label = if state.paused {
            "재생 /"
        } else {
            "일시정지 /"
        };
        if self.play_pause.label().as_deref() != Some(label) {
            self.play_pause.set_label(label);
        }
        self.speed.set_sensitive(state.active);
        self.frame_target.set_sensitive(state.can_jump());
        self.frame_jump.set_sensitive(state.can_jump());
        self.frame_target.set_range(
            1.0,
            f64::from(state.total.unwrap_or(ncat_protocol::MAX_ANIMATION_FRAMES)),
        );
    }

    fn jump_frame(self: &Rc<Self>) {
        let state = self.playback.get();
        if !state.can_jump() {
            return;
        }
        if state.busy {
            self.status
                .set_text("프레임을 읽는 중입니다. 표시된 뒤 다시 이동하세요");
            return;
        }
        self.frame_target.update();
        let index = self.frame_target.value_as_int().max(1) as u32 - 1;
        self.request_animation(Some(index));
    }

    fn request_animation(self: &Rc<Self>, seek: Option<u32>) {
        let mut state = self.playback.get();
        if !state.active || state.busy || self.closing.get() {
            return;
        }
        let Some(book) = self.book.borrow().clone() else {
            return;
        };
        self.stop_animation_timer();
        let result = if let Some(frame) = seek {
            self.backend
                .animation_seek(book, self.selected.get(), self.animation_view.get(), frame)
        } else {
            self.backend
                .animation_next(book, self.selected.get(), self.animation_view.get())
        };
        state.busy = result.is_ok();
        self.playback.set(state);
        self.update_playback();
    }

    fn schedule_animation(self: &Rc<Self>) {
        self.stop_animation_timer();
        let state = self.playback.get();
        if !state.active
            || state.paused
            || state.ended
            || state.busy
            || (self.smoke && !self.smoke_animation)
        {
            return;
        }
        let generation = self.backend.generation();
        let weak = Rc::downgrade(self);
        *self.animation_timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(state.delay()),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.animation_timer.borrow_mut().take();
                    if !ui.closing.get() && ui.backend.generation() == generation {
                        ui.request_animation(None);
                    }
                }
            },
        ));
    }

    fn step_frame(self: &Rc<Self>, forward: bool) {
        let mut state = self.playback.get();
        if !state.active {
            return;
        }
        state.paused = true;
        self.playback.set(state);
        self.stop_animation_timer();
        self.update_playback();
        if state.busy {
            return;
        }
        if forward {
            if !state.ended {
                self.request_animation(None);
            }
        } else if state.index > 0 {
            self.request_animation(Some(state.index - 1));
        }
    }

    fn toggle_playback(self: &Rc<Self>) {
        let mut state = self.playback.get();
        if !state.active {
            return;
        }
        state.paused = !state.paused;
        self.playback.set(state);
        self.update_playback();
        if !state.paused && state.ended && !state.busy {
            self.request_animation(Some(0));
        } else {
            self.schedule_animation();
        }
    }

    fn focus_viewer(&self) {
        self.interface_input.set(false);
        self.picture.grab_focus();
    }

    fn update_interface(&self) {
        let visible = interface_shown(self.interface_visible.get(), self.window.is_fullscreen());
        if !visible {
            self.focus_viewer();
        }
        self.header.set_visible(visible);
        self.toolbar.set_visible(visible);
    }

    fn interface_has_input(&self) -> bool {
        self.interface_input.get()
            && gtk::prelude::GtkWindowExt::focus(&self.window).is_some_and(|focus| {
                focus.is_ancestor(&self.toolbar) || focus.is_ancestor(&self.header)
            })
    }

    fn view(&self) -> View {
        let scale = f64::from(self.window.scale_factor());
        let w = self.scroll.width().max(320);
        let h = self.scroll.height().max(240);
        let state = self.reader_view.get();
        let (width, height) = state.render_size((w, h), scale, ncat_protocol::MAX_VIEW_DIMENSION);
        View {
            width,
            height,
            quarter_turns: state.quarter_turns,
        }
    }

    fn loading(self: &Rc<Self>) {
        self.status.set_text("읽는 중 · 이전 페이지 표시 중");
        if let Some(timer) = self.loading_timer.borrow_mut().take() {
            timer.remove();
        }
        let weak = Rc::downgrade(self);
        let generation = self.backend.generation();
        *self.loading_timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(500),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.loading_timer.borrow_mut().take();
                    if !ui.closing.get() && ui.backend.generation() == generation {
                        ui.picture.set_paintable(gdk::Paintable::NONE);
                        ui.status.set_text("페이지를 읽는 중…");
                    }
                }
            },
        ));
    }

    fn open(self: &Rc<Self>, path: PathBuf) {
        self.open_file(gio::File::for_commandline_arg(path), false);
    }

    fn cancel_network(&self) {
        let request = self.network_request.get();
        self.network_request.set(request.wrapping_add(1));
        if let Some(timer) = self.network_timer.borrow_mut().take() {
            timer.remove();
        }
        if let Some(task) = self.network_task.borrow_mut().take() {
            task.abort();
            self.backend
                .diagnose_drop(DropEvent::NetworkCancelled { request });
        }
    }

    fn open_file(self: &Rc<Self>, file: gio::File, from_drop: bool) -> bool {
        self.cancel_network();
        let location = match network::classify(file) {
            Ok(location) => location,
            Err(reason) => {
                self.backend.diagnose_drop(DropEvent::NetworkFailed {
                    request: self.network_request.get(),
                    reason,
                });
                if from_drop && reason == NetworkFailure::UnsupportedScheme {
                    self.backend
                        .diagnose_drop(DropEvent::Rejected(DropRejection::NonLocal));
                }
                self.status.set_text(network::message(reason));
                return false;
            }
        };
        let network::Location::Remote { file, protocol } = location else {
            if let network::Location::Local(path) = location {
                self.open_source(path, from_drop);
            }
            return true;
        };
        self.cancel_reload();
        if let Some(timer) = self.loading_timer.borrow_mut().take() {
            timer.remove();
        }
        if let Some(generation) = self.drop_generation.take() {
            self.backend
                .diagnose_drop(DropEvent::Superseded { generation });
        }
        self.backend.cancel();
        let request = self.network_request.get();
        self.backend
            .diagnose_drop(DropEvent::NetworkResolving { request, protocol });
        self.status
            .set_text("네트워크 폴더 연결 중… 인증이 필요하면 파일 관리자에서 먼저 로그인하세요");
        let parent = self.window.clone();
        let resolve = self.resolve_network;
        let weak = Rc::downgrade(self);
        *self.network_task.borrow_mut() =
            Some(glib::MainContext::default().spawn_local(async move {
                let result = resolve(file, parent).await;
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                if ui.closing.get() || ui.network_request.get() != request {
                    return;
                }
                ui.network_task.borrow_mut().take();
                if let Some(timer) = ui.network_timer.borrow_mut().take() {
                    timer.remove();
                }
                match result {
                    Ok((path, bridge)) => {
                        ui.backend
                            .diagnose_drop(DropEvent::NetworkResolved { request, bridge });
                        ui.open_source(path, true);
                    }
                    Err(reason) => {
                        ui.backend
                            .diagnose_drop(DropEvent::NetworkFailed { request, reason });
                        ui.status.set_text(network::message(reason));
                    }
                }
            }));
        let weak = Rc::downgrade(self);
        *self.network_timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_secs(60),
            move || {
                if let Some(ui) = weak.upgrade()
                    && ui.network_request.get() == request
                {
                    ui.network_timer.borrow_mut().take();
                    ui.cancel_network();
                    ui.backend.diagnose_drop(DropEvent::NetworkFailed {
                        request,
                        reason: NetworkFailure::Timeout,
                    });
                    ui.status
                        .set_text(network::message(NetworkFailure::Timeout));
                }
            },
        ));
        self.focus_viewer();
        true
    }

    fn open_source(self: &Rc<Self>, path: PathBuf, from_drop: bool) {
        self.cancel_network();
        if let Some(generation) = self.drop_generation.take() {
            self.backend
                .diagnose_drop(DropEvent::Superseded { generation });
        }
        self.cancel_reload();
        self.book.borrow_mut().take();
        self.reset_playback(true);
        self.picture.set_paintable(gdk::Paintable::NONE);
        self.selected.set(0);
        self.reader_view.set(ReaderView::default());
        self.layout_picture();
        self.animation_view.set(self.view());
        match self.backend.open(path, self.animation_view.get()) {
            Ok(generation) => {
                if from_drop {
                    self.drop_generation.set(Some(generation));
                    self.backend
                        .diagnose_drop(DropEvent::OpenQueued { generation });
                }
                self.loading();
            }
            Err(code) => {
                self.status.set_text(&code.to_string());
                if from_drop {
                    self.backend.diagnose_drop(DropEvent::OpenFailed {
                        generation: self.backend.generation(),
                        code,
                    });
                }
            }
        }
        self.update_navigation();
        self.focus_viewer();
    }

    fn cancel_reload(&self) {
        if let Some(timer) = self.animation_timer.borrow_mut().take() {
            timer.remove();
        }
        if let Some(timer) = self.reload_timer.borrow_mut().take() {
            timer.remove();
        }
    }

    fn load(self: &Rc<Self>, index: usize) {
        self.cancel_network();
        self.cancel_reload();
        let book = self.book.borrow().clone();
        if let Some(book) = book {
            if !book.complete || index >= book.pages.len() {
                return;
            }
            self.reset_playback(index != self.selected.get());
            self.selected.set(index);
            self.animation_view.set(self.view());
            if self
                .backend
                .load(book, index, self.animation_view.get())
                .is_ok()
            {
                self.loading();
            }
            self.update_navigation();
        }
    }

    fn navigate(self: &Rc<Self>, forward: bool) {
        let len = self
            .book
            .borrow()
            .as_ref()
            .filter(|book| book.complete)
            .map_or(0, |book| book.pages.len());
        if len == 0 {
            return;
        }
        let index = adjacent(self.selected.get(), len, forward).unwrap();
        if index != self.selected.get() {
            let mut state = self.reader_view.get();
            state.zoom = 1.0;
            state.quarter_turns = 0;
            self.reader_view.set(state);
            self.layout_picture();
            self.load(index);
        } else {
            self.status.set_text(if forward {
                "마지막 페이지입니다"
            } else {
                "첫 페이지입니다"
            });
        }
        self.focus_viewer();
    }

    fn update_navigation(&self) {
        self.syncing.set(true);
        let book = self.book.borrow();
        let len = book
            .as_ref()
            .filter(|b| b.complete)
            .map_or(0, |b| b.pages.len());
        self.previous
            .set_sensitive(len > 0 && self.selected.get() > 0);
        self.next
            .set_sensitive(len > 0 && self.selected.get() + 1 < len);
        self.position.set_sensitive(len > 0);
        self.position.set_range(1.0, len.max(1) as f64);
        self.position.set_value((self.selected.get() + 1) as f64);
        if let Some(book) = book.as_ref() {
            let name = book
                .pages
                .get(self.selected.get())
                .map_or("", |p| p.name.as_str());
            let count = if book.complete {
                book.pages.len().to_string()
            } else {
                "분석 중".into()
            };
            self.window.set_title(Some(&format!(
                "{} / {} · {} — nCatImage",
                self.selected.get() + 1,
                count,
                name
            )));
        }
        self.syncing.set(false);
    }

    fn layout_picture(&self) {
        let size = self
            .reader_view
            .get()
            .picture_size((self.scroll.width(), self.scroll.height()));
        if let Some((w, h)) = size {
            self.scroll
                .set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
            self.picture.set_size_request(w, h);
            self.picture.set_content_fit(gtk::ContentFit::Contain);
        } else {
            self.scroll
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
            self.picture.set_size_request(-1, -1);
            self.picture.set_content_fit(gtk::ContentFit::ScaleDown);
        }
    }

    fn change_zoom(self: &Rc<Self>, factor: f64) {
        if self.book.borrow().is_none() {
            return;
        }
        let mut state = self.reader_view.get();
        state.change_zoom(factor);
        self.reader_view.set(state);
        self.layout_picture();
        self.cancel_reload();
        self.backend.cancel();
        let weak = Rc::downgrade(self);
        *self.reload_timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(100),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.reload_timer.borrow_mut().take();
                    ui.load(ui.selected.get());
                }
            },
        ));
    }

    fn rotate(self: &Rc<Self>, backwards: bool) {
        let mut state = self.reader_view.get();
        state.rotate(backwards);
        self.reader_view.set(state);
        self.load(self.selected.get());
    }

    fn dialog(self: &Rc<Self>, folder: bool) {
        let dialog = gtk::FileDialog::builder()
            .title(if folder {
                "만화 폴더 열기"
            } else {
                "이미지 또는 ZIP/CBZ 열기"
            })
            .build();
        let weak = Rc::downgrade(self);
        if folder {
            dialog.select_folder(Some(&self.window), gio::Cancellable::NONE, move |result| {
                if let (Some(ui), Ok(file)) = (weak.upgrade(), result) {
                    ui.open_file(file, false);
                }
            });
        } else {
            dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
                if let (Some(ui), Ok(file)) = (weak.upgrade(), result) {
                    ui.open_file(file, false);
                }
            });
        }
    }

    fn network_dialog(self: &Rc<Self>) {
        let dialog = gtk::Window::builder()
            .title("네트워크 파일 또는 폴더 열기")
            .transient_for(&self.window)
            .modal(true)
            .destroy_with_parent(true)
            .build();
        if let Some(app) = self.window.application() {
            dialog.set_application(Some(&app));
        }
        let cancel = gtk::Button::with_label("취소");
        let open = gtk::Button::with_label("열기");
        dialog.set_default_widget(Some(&open));
        let entry = gtk::Entry::builder()
            .placeholder_text("smb://서버/공유/폴더 또는 ftp://… / sftp://…")
            .input_purpose(gtk::InputPurpose::Url)
            .activates_default(true)
            .width_chars(56)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.append(&entry);
        let hint = gtk::Label::new(Some(
            "인증은 OS 연결 도구가 처리합니다. 비밀번호를 URL에 넣지 마세요. FTP는 암호화되지 않습니다.",
        ));
        hint.set_wrap(true);
        hint.set_max_width_chars(56);
        content.append(&hint);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        actions.append(&cancel);
        actions.append(&open);
        content.append(&actions);
        dialog.set_child(Some(&content));
        let weak = Rc::downgrade(self);
        let weak_dialog = dialog.downgrade();
        open.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.open_file(gio::File::for_uri(entry.text().trim()), false);
            }
            if let Some(dialog) = weak_dialog.upgrade() {
                dialog.close();
            }
        });
        let weak_dialog = dialog.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = weak_dialog.upgrade() {
                dialog.close();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak_dialog = dialog.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(dialog) = weak_dialog.upgrade() {
                    dialog.close();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        dialog.add_controller(keys);
        dialog.present();
    }

    fn consume(self: &Rc<Self>, event: Event) {
        if self.closing.get() || event.generation() != self.backend.generation() {
            return;
        }
        if let Some(generation) = self.drop_generation.get()
            && generation != event.generation()
        {
            self.drop_generation.set(None);
            self.backend
                .diagnose_drop(DropEvent::Superseded { generation });
        }
        match event {
            Event::Catalog { book, .. } => {
                self.selected.set(book.selected);
                *self.book.borrow_mut() = Some(book);
                self.update_navigation();
            }
            Event::Frame { frame, cached, .. } => {
                if let Some(timer) = self.loading_timer.borrow_mut().take() {
                    timer.remove();
                }
                let info = frame.info.clone();
                if !info.valid() {
                    return;
                }
                let bytes = glib::Bytes::from_owned(FrameBytes(frame));
                let texture = gdk::MemoryTexture::new(
                    info.width as i32,
                    info.height as i32,
                    gdk::MemoryFormat::R8g8b8a8,
                    &bytes,
                    info.stride as usize,
                );
                // GBytes owns FrameBytes, so the reservation survives the GUI's local variables.
                self.picture.set_paintable(Some(&texture));
                if let Some(generation) = self.drop_generation.take() {
                    self.backend
                        .diagnose_drop(DropEvent::Displayed { generation });
                }
                let mut state = self.reader_view.get();
                state.original = (info.original_width, info.original_height);
                self.reader_view.set(state);
                self.layout_picture();
                if let Some(animation) = &info.animation {
                    let mut state = self.playback.get();
                    state.displayed(animation.index, animation.cycle, animation.delay_ms);
                    if let Some(total) = animation.total {
                        state.total = Some(total);
                    }
                    self.playback.set(state);
                    self.update_playback();
                    self.frame_target.set_value(f64::from(state.index + 1));
                    self.schedule_animation();
                } else {
                    self.reset_playback(true);
                }
                self.status.set_text(&format!(
                    "{} · {}×{} · {} · {:.1} MiB 예약 · M1 축소 표시 / ICC 보정 미구현",
                    info.format,
                    info.original_width,
                    info.original_height,
                    if cached { "캐시 적중" } else { "디코딩" },
                    self.backend.budget_used() as f64 / 1_048_576.0
                ));
                if self.smoke {
                    if self.smoke_animation {
                        if info.animation.is_none() {
                            self.window.close();
                            return;
                        }
                        let count = self.smoke_animation_frames.get() + 1;
                        self.smoke_animation_frames.set(count);
                        if self.smoke_controls {
                            self.controls_smoke(count);
                            return;
                        }
                        if count < 6 {
                            return;
                        }
                        self.cancel_reload();
                        println!("animation_smoke=ok displayed_frames={count}");
                    }
                    if let Some(remaining) = self.smoke_steps.get() {
                        if remaining > 0 {
                            self.start_navigation_smoke();
                            return;
                        }
                        println!(
                            "navigation_smoke=ok reserved={} stats={:?}",
                            self.backend.budget_used(),
                            self.backend.cache_stats()
                        );
                    }
                    self.smoke_ok.set(true);
                    let weak = Rc::downgrade(self);
                    let hold = if self.smoke_steps.get().is_some() {
                        Duration::from_secs(5)
                    } else {
                        Duration::from_millis(150)
                    };
                    glib::timeout_add_local_once(hold, move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.window.close();
                        }
                    });
                }
            }
            Event::Error { code, .. } => {
                self.reset_playback(true);
                if let Some(timer) = self.animation_timer.borrow_mut().take() {
                    timer.remove();
                }
                if let Some(generation) = self.drop_generation.take() {
                    self.backend
                        .diagnose_drop(DropEvent::OpenFailed { generation, code });
                }
                if let Some(timer) = self.loading_timer.borrow_mut().take() {
                    timer.remove();
                }
                self.picture.set_paintable(gdk::Paintable::NONE);
                self.status.set_text(&code.to_string());
                if self.smoke {
                    self.smoke_ok.set(false);
                    eprintln!("nCatImage: smoke failure: {code:?}");
                    self.window.close();
                }
            }
            Event::AnimationEnded { .. } => {
                let mut state = self.playback.get();
                state.finish();
                self.playback.set(state);
                self.stop_animation_timer();
                self.update_playback();
                if self.smoke_controls {
                    assert_eq!(self.smoke_animation_frames.get(), 4);
                    assert!(self.playback.get().ended && self.playback.get().paused);
                    self.play_pause.emit_clicked();
                    return;
                }
                if self.smoke_animation {
                    let count = self.smoke_animation_frames.get();
                    self.smoke_ok.set(count >= 2);
                    if count >= 2 {
                        println!("animation_smoke=ok displayed_frames={count} finite_end=true");
                    }
                    self.window.close();
                }
            }
        }
    }

    // Explicit test mode only; no timer or automatic navigation in normal reading.
    fn start_navigation_smoke(self: &Rc<Self>) {
        if self.smoke_started.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(5), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if ui.closing.get() {
                return glib::ControlFlow::Break;
            }
            let remaining = ui.smoke_steps.get().unwrap_or(0);
            if remaining == 0 {
                return glib::ControlFlow::Break;
            }
            let len = ui.book.borrow().as_ref().map_or(0, |book| book.pages.len());
            if len == 0 {
                return glib::ControlFlow::Continue;
            }
            ui.smoke_steps.set(Some(remaining - 1));
            ui.load((ui.selected.get() + 1) % len);
            if remaining == 1 {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    fn close(&self) {
        if self.closing.replace(true) {
            return;
        }
        self.cancel_network();
        self.cancel_reload();
        if let Some(timer) = self.loading_timer.borrow_mut().take() {
            timer.remove();
        }
        self.backend.shutdown();
        self.book.borrow_mut().take();
        self.picture.set_paintable(gdk::Paintable::NONE);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SmokeMode {
    None,
    Empty,
    Navigation,
    Frame,
    Animation,
    AnimationControls,
}

fn build(
    app: &gtk::Application,
    initial: Option<PathBuf>,
    mode: SmokeMode,
    smoke_ok: Rc<Cell<bool>>,
    resolve_network: network::Resolver,
) {
    let smoke = mode != SmokeMode::None;
    let smoke_empty = mode == SmokeMode::Empty;
    let smoke_navigation = mode == SmokeMode::Navigation;
    let backend = match ncat_backend::BackendOptions::from_env()
        .and_then(|options| worker_path().and_then(|path| Backend::with_options(path, options)))
    {
        Ok(backend) => backend,
        Err(_) => {
            eprintln!("nCatImage: backend initialization failed");
            app.quit();
            return;
        }
    };
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(concat!("nCatImage · v", env!("CARGO_PKG_VERSION")))
        .default_width(1000)
        .default_height(800)
        .build();
    let header = gtk::HeaderBar::new();
    let open = gtk::Button::with_label("파일 열기");
    let folder = gtk::Button::with_label("폴더");
    let network = gtk::Button::with_label("네트워크");
    network.set_tooltip_text(Some("SMB / FTP / SFTP 주소 열기 · Ctrl+L"));
    header.pack_start(&open);
    header.pack_start(&folder);
    header.pack_start(&network);
    let direction = gtk::ToggleButton::with_label("우→좌");
    direction.set_tooltip_text(Some("좌우 방향키의 읽기 방향 · D"));
    header.pack_end(&direction);
    let fullscreen = gtk::Button::with_label("전체 화면");
    header.pack_end(&fullscreen);
    window.set_titlebar(Some(&header));
    let frame_status = gtk::Label::new(Some("애니메이션 없음"));
    frame_status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    frame_status.set_width_chars(24);
    frame_status.set_max_width_chars(24);
    frame_status.set_xalign(0.0);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let picture = gtk::Picture::builder()
        .hexpand(true)
        .vexpand(true)
        .can_shrink(true)
        .focusable(true)
        .build();
    picture.set_alternative_text(Some("현재 만화 페이지"));
    let scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&picture)
        .build();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
    let toolbar = gtk::Box::new(gtk::Orientation::Vertical, 6);
    toolbar.set_margin_top(6);
    toolbar.set_margin_bottom(8);
    let status = gtk::Label::new(Some(
        "파일/폴더/ZIP을 열거나 드롭하세요 · ↑ ↓ / Space · ` UI 토글 · Esc 종료",
    ));
    status.set_wrap(true);
    status.set_margin_start(12);
    status.set_margin_end(12);
    toolbar.append(&status);
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    controls.set_halign(gtk::Align::Center);
    let previous = gtk::Button::with_label("이전");
    let next = gtk::Button::with_label("다음");
    let position = gtk::SpinButton::with_range(1.0, 1.0, 1.0);
    position.set_numeric(true);
    position.set_width_chars(6);
    let smaller = gtk::Button::with_label("−");
    let fit = gtk::Button::with_label("맞춤");
    let larger = gtk::Button::with_label("+");
    let rotate = gtk::Button::with_label("회전");
    for widget in [
        previous.upcast_ref::<gtk::Widget>(),
        position.upcast_ref(),
        next.upcast_ref(),
        smaller.upcast_ref(),
        fit.upcast_ref(),
        larger.upcast_ref(),
        rotate.upcast_ref(),
    ] {
        controls.append(widget);
    }
    toolbar.append(&controls);
    let animation_controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    animation_controls.set_halign(gtk::Align::Center);
    let frame_previous = gtk::Button::with_label("이전 프레임 ,");
    let frame_next = gtk::Button::with_label("다음 프레임 .");
    let play_pause = gtk::Button::with_label("일시정지 /");
    let frame_target =
        gtk::SpinButton::with_range(1.0, f64::from(ncat_protocol::MAX_ANIMATION_FRAMES), 1.0);
    frame_target.set_numeric(true);
    frame_target.set_width_chars(5);
    frame_target.set_tooltip_text(Some("일시정지 상태에서 이동할 프레임 번호 (1부터 시작)"));
    let frame_jump = gtk::Button::with_label("이동");
    frame_jump.set_tooltip_text(Some("지정한 프레임으로 이동 · Enter"));
    frame_previous.set_tooltip_text(Some("일시정지 후 이전 프레임 · ,"));
    frame_next.set_tooltip_text(Some("일시정지 후 다음 프레임 · ."));
    play_pause.set_tooltip_text(Some("재생 / 일시정지 · /"));
    let speed = gtk::DropDown::from_strings(&[
        "0.25×", "0.5×", "0.75×", "1×", "1.25×", "1.5×", "2×", "3×", "4×",
    ]);
    speed.set_selected(3);
    speed.set_tooltip_text(Some("애니메이션 재생 속도 (0.25×~4×)"));
    animation_controls.append(&frame_status);
    animation_controls.append(&frame_previous);
    animation_controls.append(&frame_next);
    animation_controls.append(&play_pause);
    animation_controls.append(&gtk::Label::new(Some("속도")));
    animation_controls.append(&speed);
    animation_controls.append(&frame_target);
    animation_controls.append(&frame_jump);
    toolbar.append(&animation_controls);
    root.append(&toolbar);
    root.append(&scroll);
    window.set_child(Some(&root));
    let ui = Rc::new(Ui {
        window,
        header,
        toolbar,
        interface_visible: Cell::new(true),
        interface_input: Cell::new(false),
        drop_generation: Cell::new(None),
        network_request: Cell::new(0),
        network_task: RefCell::new(None),
        resolve_network,
        network_timer: RefCell::new(None),
        picture,
        scroll,
        status,
        position,
        previous,
        next,
        direction_button: direction,
        backend,
        book: RefCell::new(None),
        selected: Cell::new(0),
        reader_view: Cell::new(ReaderView::default()),
        syncing: Cell::new(false),
        closing: Cell::new(false),
        reload_timer: RefCell::new(None),
        loading_timer: RefCell::new(None),
        animation_timer: RefCell::new(None),
        animation_view: Cell::new(View::default()),
        playback: Cell::new(Playback::default()),
        frame_status,
        frame_previous,
        frame_next,
        frame_target,
        frame_jump,
        play_pause,
        speed,
        smoke,
        smoke_animation: matches!(mode, SmokeMode::Animation | SmokeMode::AnimationControls),
        smoke_controls: mode == SmokeMode::AnimationControls,
        smoke_animation_frames: Cell::new(0),
        smoke_sensitivity_changes: Cell::new(0),
        smoke_steps: Cell::new(smoke_navigation.then_some(500)),
        smoke_started: Cell::new(false),
        smoke_ok,
    });
    ui.update_navigation();
    ui.update_playback();
    if ui.smoke_controls {
        for button in [&ui.frame_previous, &ui.frame_next] {
            let weak = Rc::downgrade(&ui);
            button.connect_sensitive_notify(move |_| {
                if let Some(ui) = weak.upgrade()
                    && ui.smoke_animation_frames.get() > 0
                {
                    ui.smoke_sensitivity_changes
                        .set(ui.smoke_sensitivity_changes.get() + 1);
                }
            });
        }
    }
    {
        let weak = Rc::downgrade(&ui);
        ui.frame_jump.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.jump_frame();
            }
        });
        let enter = gtk::EventControllerKey::new();
        enter.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&ui);
        enter.connect_key_pressed(move |_, key, _, _| {
            if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
                if let Some(ui) = weak.upgrade() {
                    ui.jump_frame();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        ui.frame_target.add_controller(enter);
        let weak = Rc::downgrade(&ui);
        ui.frame_previous.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.step_frame(false);
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.frame_next.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.step_frame(true);
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.play_pause.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.toggle_playback();
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.speed.connect_selected_notify(move |combo| {
            if let Some(ui) = weak.upgrade()
                && let Some(value) = ncat_presentation::playback::SPEEDS
                    .get(combo.selected() as usize)
                    .copied()
            {
                let mut state = ui.playback.get();
                state.set_speed(value);
                ui.playback.set(state);
                ui.update_playback();
                ui.schedule_animation();
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        network.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.network_dialog();
            }
        });
    }
    // Only an explicit pointer click opts controls into keyboard input. Merely
    // showing/restoring a window or pressing Tab must not focus the page spin.
    for widget in [
        ui.header.upcast_ref::<gtk::Widget>(),
        ui.toolbar.upcast_ref(),
    ] {
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&ui);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                ui.interface_input.set(true);
            }
        });
        widget.add_controller(click);
    }
    {
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&ui);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                ui.focus_viewer();
            }
        });
        ui.scroll.add_controller(click);
    }
    {
        let weak = Rc::downgrade(&ui);
        ui.window.connect_fullscreened_notify(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.update_interface();
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        open.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.dialog(false);
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        folder.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.dialog(true);
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        fullscreen.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                if ui.window.is_fullscreen() {
                    ui.window.unfullscreen();
                } else {
                    ui.window.fullscreen();
                }
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        ui.previous.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.navigate(false);
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        ui.next.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.navigate(true);
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        smaller.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.change_zoom(1.0 / 1.25);
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        larger.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.change_zoom(1.25);
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        fit.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.change_zoom(0.0);
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        rotate.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.rotate(false);
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        ui.position.connect_value_changed(move |position| {
            if let Some(ui) = weak.upgrade()
                && !ui.syncing.get()
            {
                ui.load(position.value_as_int().saturating_sub(1) as usize);
            }
        });
    }
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let weak = Rc::downgrade(&ui);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(ui) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            use ncat_presentation::input::{Command, Modifiers, command};
            let action = command(
                input::key(key),
                Modifiers {
                    primary: modifiers.contains(gdk::ModifierType::CONTROL_MASK),
                    shift: modifiers.contains(gdk::ModifierType::SHIFT_MASK),
                    alternate: modifiers.contains(gdk::ModifierType::ALT_MASK),
                    system: modifiers.contains(gdk::ModifierType::SUPER_MASK),
                },
                ui.interface_has_input(),
            );
            match action {
                Command::FramePrevious => ui.step_frame(false),
                Command::FrameNext => ui.step_frame(true),
                Command::Playback => ui.toggle_playback(),
                Command::Native => return glib::Propagation::Proceed,
                Command::Block => {}
                Command::Open { folder } => ui.dialog(folder),
                Command::Network => ui.network_dialog(),
                Command::Quit => ui.window.close(),
                Command::Fullscreen => {
                    if ui.window.is_fullscreen() {
                        ui.window.unfullscreen();
                    } else {
                        ui.window.fullscreen();
                    }
                }
                Command::ToggleInterface => {
                    if !ui.window.is_fullscreen() {
                        ui.interface_visible.set(!ui.interface_visible.get());
                        ui.update_interface();
                    }
                }
                Command::Page(key) => {
                    ui.focus_viewer();
                    let direction = if ui.direction_button.is_active() {
                        Direction::RightToLeft
                    } else {
                        Direction::LeftToRight
                    };
                    ui.navigate(ncat_presentation::page_step(key, direction));
                }
                Command::First => {
                    ui.focus_viewer();
                    ui.load(0);
                }
                Command::Last => {
                    ui.focus_viewer();
                    let len = ui.book.borrow().as_ref().map_or(0, |b| b.pages.len());
                    if len > 0 {
                        ui.load(len - 1);
                    }
                }
                Command::ZoomIn => {
                    ui.focus_viewer();
                    ui.change_zoom(1.25);
                }
                Command::ZoomOut => {
                    ui.focus_viewer();
                    ui.change_zoom(1.0 / 1.25);
                }
                Command::Fit => {
                    ui.focus_viewer();
                    ui.change_zoom(0.0);
                }
                Command::Rotate { backwards } => {
                    ui.focus_viewer();
                    ui.rotate(backwards);
                }
                Command::Direction => ui
                    .direction_button
                    .set_active(!ui.direction_button.is_active()),
                Command::Reload => {
                    ui.focus_viewer();
                    let source = ui.book.borrow().as_ref().map(|b| b.source.path());
                    if let Some(source) = source {
                        ui.open(source);
                    }
                }
            }
            glib::Propagation::Stop
        });
    }
    ui.window.add_controller(keys);

    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    {
        let weak = Rc::downgrade(&ui);
        let accumulated = Cell::new(0.0_f64);
        let last_page = Cell::new(None::<std::time::Instant>);
        wheel.connect_scroll(move |controller, _, dy| {
            let Some(ui) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if controller
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK)
            {
                if dy != 0.0 {
                    ui.change_zoom(if dy < 0.0 { 1.1 } else { 1.0 / 1.1 });
                }
                return glib::Propagation::Stop;
            }
            if ui.reader_view.get().zoom > 1.0 {
                return glib::Propagation::Proceed;
            }
            let old = accumulated.get();
            accumulated.set(if old.signum() != dy.signum() {
                dy
            } else {
                old + dy
            });
            if accumulated.get().abs() >= 1.0
                && last_page
                    .get()
                    .is_none_or(|last| last.elapsed() >= Duration::from_millis(100))
            {
                ui.navigate(accumulated.get() > 0.0);
                accumulated.set(0.0);
                last_page.set(Some(std::time::Instant::now()));
            }
            glib::Propagation::Stop
        });
    }
    ui.picture.add_controller(wheel);

    let drag = gtk::GestureDrag::new();
    let drag_origin = Rc::new(Cell::new((0.0, 0.0)));
    {
        let weak = Rc::downgrade(&ui);
        let origin = drag_origin.clone();
        drag.connect_drag_begin(move |_, _, _| {
            if let Some(ui) = weak.upgrade() {
                origin.set((
                    ui.scroll.hadjustment().value(),
                    ui.scroll.vadjustment().value(),
                ));
                ui.focus_viewer();
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some(ui) = weak.upgrade()
                && ui.reader_view.get().zoom > 1.0
            {
                let (x, y) = drag_origin.get();
                ui.scroll.hadjustment().set_value(x - dx);
                ui.scroll.vadjustment().set_value(y - dy);
            }
        });
    }
    ui.picture.add_controller(drag);

    // Prefer the file-list representation so count/locality can be validated,
    // while retaining GFile compatibility for sources offering a single file.
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    drop.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);
    drop.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let weak = Rc::downgrade(&ui);
        drop.connect_accept(move |_, offered| {
            let formats = offered.formats().union_deserialize_types();
            let compatible = formats.contains_type(gdk::FileList::static_type())
                || formats.contains_type(gio::File::static_type());
            if let Some(ui) = weak.upgrade() {
                ui.backend.diagnose_drop(DropEvent::Accepted { compatible });
            }
            compatible
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        drop.connect_drop(move |_, value, _, _| {
            let Some(ui) = weak.upgrade() else {
                return false;
            };
            let (files, file_list) = if let Ok(list) = value.get::<gdk::FileList>() {
                (list.files(), true)
            } else if let Ok(file) = value.get::<gio::File>() {
                (vec![file], false)
            } else {
                ui.backend
                    .diagnose_drop(DropEvent::Rejected(DropRejection::UnsupportedValue));
                ui.status.set_text("지원하지 않는 드롭 형식입니다");
                return false;
            };
            ui.backend.diagnose_drop(DropEvent::Received {
                file_list,
                items: files.len(),
            });
            match single_drop_file(&files) {
                Ok(file) => ui.open_file(file.clone(), true),
                Err(reason) => {
                    ui.backend.diagnose_drop(DropEvent::Rejected(reason));
                    ui.status.set_text(match reason {
                        DropRejection::Empty => "드롭된 파일이 없습니다",
                        DropRejection::MultipleItems => "파일 또는 폴더를 한 개씩 드롭하세요",
                        DropRejection::NonLocal => "로컬 또는 SMB·FTP·SFTP 위치만 열 수 있습니다",
                        DropRejection::UnsupportedValue => "지원하지 않는 드롭 형식입니다",
                    });
                    false
                }
            }
        });
    }
    ui.window.add_controller(drop);
    // Observe even unsupported offers, before data negotiation. Do not log
    // motion events, URI strings, file names, paths or arbitrary MIME strings.
    let drag_observer = gtk::DropControllerMotion::new();
    drag_observer.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let weak = Rc::downgrade(&ui);
        drag_observer.connect_enter(move |controller, _, _| {
            if let Some(ui) = weak.upgrade()
                && let Some(offered) = controller.drop()
            {
                let formats = offered.formats();
                ui.backend.diagnose_drop(DropEvent::Entered {
                    file_list: formats.contains_type(gdk::FileList::static_type()),
                    file: formats.contains_type(gio::File::static_type()),
                    uri_list: formats.contain_mime_type("text/uri-list"),
                    portal: formats.contain_mime_type("application/vnd.portal.filetransfer")
                        || formats.contain_mime_type("application/vnd.portal.files"),
                    actions: offered.actions().bits(),
                    wayland: ui.picture.display().type_().name().contains("Wayland"),
                });
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        drag_observer.connect_leave(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.backend.diagnose_drop(DropEvent::Left);
            }
        });
    }
    ui.window.add_controller(drag_observer);
    ui.backend.diagnose_drop(DropEvent::Ready {
        wayland: ui.picture.display().type_().name().contains("Wayland"),
    });
    {
        let weak = Rc::downgrade(&ui);
        ui.window.connect_close_request(move |_| {
            if let Some(ui) = weak.upgrade() {
                if ui.closing.get() {
                    return glib::Propagation::Stop;
                }
                ui.close();
                let started = std::time::Instant::now();
                // Bounded shutdown-only check, not a steady-state result polling loop.
                // Keep Ui alive until the coordinator has killed/reaped the worker.
                glib::timeout_add_local(Duration::from_millis(10), move || {
                    if ui.backend.is_closed() || started.elapsed() >= Duration::from_millis(1500) {
                        ui.window.destroy();
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    // Awaiting this bounded channel wakes the main context; no idle timer/poll loop.
    let events = ui.backend.events();
    let receiver_ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        while let Ok(event) = events.recv().await {
            receiver_ui.consume(event);
            // Yield between results so queued frames cannot upload in one UI dispatch.
            glib::timeout_future(Duration::from_millis(1)).await;
        }
    });
    ui.window.present();
    ui.focus_viewer();
    if let Some(path) = initial {
        let ui = ui.clone();
        glib::idle_add_local_once(move || ui.open(path));
    }
    if smoke {
        if smoke_empty {
            let weak = Rc::downgrade(&ui);
            glib::timeout_add_local_once(Duration::from_millis(500), move || {
                if let Some(ui) = weak.upgrade() {
                    ui.smoke_ok.set(ui.window.is_mapped());
                    ui.window.close();
                }
            });
        }
        let weak = Rc::downgrade(&ui);
        glib::timeout_add_local_once(
            Duration::from_secs(if smoke_navigation { 60 } else { 10 }),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.window.close();
                }
            },
        );
    }
}

pub fn run(resolve_network: network::Resolver) -> glib::ExitCode {
    std::panic::set_hook(Box::new(|_| eprintln!("nCatImage: internal failure")));
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    let smoke_empty = first.as_ref().is_some_and(|value| value == "--smoke-empty");
    let smoke_navigation = first
        .as_ref()
        .is_some_and(|value| value == "--smoke-navigation");
    let smoke_animation = first
        .as_ref()
        .is_some_and(|value| value == "--smoke-animation");
    let smoke_controls = first
        .as_ref()
        .is_some_and(|value| value == "--smoke-animation-controls");
    let smoke = smoke_empty
        || smoke_navigation
        || smoke_animation
        || smoke_controls
        || first.as_ref().is_some_and(|value| value == "--smoke-test");
    let initial = if smoke_empty {
        None
    } else if smoke {
        args.next()
    } else {
        first
    }
    .map(PathBuf::from);
    let smoke_ok = Rc::new(Cell::new(false));
    let app = gtk::Application::builder()
        .application_id("io.github.ncatimage.Development")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    {
        let smoke_ok = smoke_ok.clone();
        app.connect_activate(move |app| {
            build(
                app,
                initial.clone(),
                if smoke_controls {
                    SmokeMode::AnimationControls
                } else if smoke_animation {
                    SmokeMode::Animation
                } else if smoke_navigation {
                    SmokeMode::Navigation
                } else if smoke_empty {
                    SmokeMode::Empty
                } else if smoke {
                    SmokeMode::Frame
                } else {
                    SmokeMode::None
                },
                smoke_ok.clone(),
                resolve_network,
            )
        });
    }
    let result = app.run_with_args::<&str>(&[]);
    if smoke && !smoke_ok.get() {
        glib::ExitCode::FAILURE
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn punctuation_keys_normalize_to_frame_controls() {
        use ncat_presentation::input::Key;
        assert_eq!(input::key(gdk::Key::comma), Key::FramePrevious);
        assert_eq!(input::key(gdk::Key::period), Key::FrameNext);
        assert_eq!(input::key(gdk::Key::slash), Key::Playback);
    }

    #[test]
    fn drop_accepts_one_local_file_or_folder_without_inspecting_the_filesystem() {
        let file = gio::File::for_path("/not-read/book with spaces.cbz");
        assert_eq!(
            single_drop_file(&[file]).unwrap().path().unwrap(),
            PathBuf::from("/not-read/book with spaces.cbz")
        );
        let folder = gio::File::for_uri("file:///not-read/%ED%8F%B4%EB%8D%94");
        assert_eq!(
            single_drop_file(&[folder]).unwrap().path().unwrap(),
            PathBuf::from("/not-read/폴더")
        );
    }

    #[test]
    fn drop_rejects_empty_multiple_and_retains_remote_uris_for_resolution() {
        assert_eq!(single_drop_file(&[]), Err(DropRejection::Empty));
        let file = gio::File::for_path("/not-read/a.png");
        assert_eq!(
            single_drop_file(&[file.clone(), file]),
            Err(DropRejection::MultipleItems)
        );
        let remote = gio::File::for_uri("smb://server/share/book.cbz");
        assert_eq!(
            single_drop_file(std::slice::from_ref(&remote))
                .unwrap()
                .uri(),
            remote.uri()
        );
    }

    #[cfg(unix)]
    #[test]
    fn drop_preserves_non_utf8_local_paths() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/not-read/\xff.png".to_vec()));
        assert_eq!(
            single_drop_file(&[gio::File::for_path(&path)])
                .unwrap()
                .path()
                .unwrap(),
            path
        );
    }

    #[test]
    fn fullscreen_always_hides_interface() {
        assert!(!interface_shown(true, true));
        assert!(!interface_shown(false, true));
    }

    #[test]
    fn windowed_interface_respects_toggle() {
        assert!(interface_shown(true, false));
        assert!(!interface_shown(false, false));
    }

    #[test]
    fn vertical_keys_match_page_keys_independent_of_reading_direction() {
        use ncat_presentation::input::Key;
        for key in [gdk::Key::Down, gdk::Key::KP_Down, gdk::Key::space] {
            assert_eq!(input::key(key), input::key(gdk::Key::Page_Down));
            assert_eq!(input::key(key), Key::Next);
        }
        for key in [gdk::Key::Up, gdk::Key::KP_Up, gdk::Key::BackSpace] {
            assert_eq!(input::key(key), input::key(gdk::Key::Page_Up));
            assert_eq!(input::key(key), Key::Previous);
        }
        assert_eq!(input::key(gdk::Key::Left), Key::Left);
        assert_eq!(input::key(gdk::Key::Escape), Key::Escape);
    }
}

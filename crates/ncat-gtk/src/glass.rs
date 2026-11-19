//! Snapshot the viewer, not the desktop. Foreground widgets are separate overlays.
use gtk::{gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct Glass {
        pub source: RefCell<Option<gtk::WidgetPaintable>>,
        pub panels: RefCell<Vec<glib::WeakRef<gtk::Widget>>>,
        pub blur_draws: Cell<u64>,
        size: Cell<(i32, i32)>,
        settle: RefCell<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Glass {
        const NAME: &'static str = "NcatReaderGlass";
        type Type = super::Glass;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Glass {
        fn dispose(&self) {
            if let Some(timer) = self.settle.borrow_mut().take() {
                timer.remove();
            }
        }
    }

    impl WidgetImpl for Glass {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            if self.size.replace((width, height)) == (width, height) {
                return;
            }
            if let Some(timer) = self.settle.borrow_mut().take() {
                timer.remove();
            }
            let weak = self.obj().downgrade();
            self.settle.replace(Some(glib::timeout_add_local_once(
                std::time::Duration::from_millis(120),
                move || {
                    if let Some(widget) = weak.upgrade() {
                        widget.imp().settle.borrow_mut().take();
                        widget.queue_draw();
                    }
                },
            )));
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let width = obj.width() as f32;
            let height = obj.height() as f32;
            let source = self.source.borrow();
            for panel in self
                .panels
                .borrow()
                .iter()
                .filter_map(glib::WeakRef::upgrade)
            {
                let Some(bounds) = panel.compute_bounds(&*obj).filter(|_| panel.is_visible())
                else {
                    continue;
                };
                if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
                    continue;
                }
                let rounded = gsk::RoundedRect::from_rect(bounds, 12.0);
                snapshot.push_rounded_clip(&rounded);
                if let Some(source) = source.as_ref().filter(|_| self.settle.borrow().is_none()) {
                    // Bound the blur input to the panel and its sampling margin.
                    let sample = graphene::Rect::new(
                        bounds.x() - 28.0,
                        bounds.y() - 28.0,
                        bounds.width() + 56.0,
                        bounds.height() + 56.0,
                    );
                    snapshot.push_blur(10.0);
                    snapshot.push_clip(&sample);
                    source.snapshot(snapshot, width.into(), height.into());
                    snapshot.pop();
                    snapshot.pop();
                    self.blur_draws.set(self.blur_draws.get().saturating_add(1));
                }
                snapshot.append_color(&gdk::RGBA::new(0.09, 0.11, 0.15, 0.77), &bounds);
                snapshot.pop();
                snapshot.append_border(
                    &rounded,
                    &[1.0; 4],
                    &[gdk::RGBA::new(0.8, 0.9, 1.0, 0.24); 4],
                );
            }
        }
    }
}

glib::wrapper! {
    pub struct Glass(ObjectSubclass<imp::Glass>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Glass {
    pub fn smoke_snapshot(&self) -> bool {
        let snapshot = gtk::Snapshot::new();
        let before = self.imp().blur_draws.get();
        // Build the same render node even if the compositor throttles this
        // test window because another window currently covers it.
        self.imp().snapshot(&snapshot);
        self.imp().blur_draws.get() > before && snapshot.to_node().is_some()
    }

    pub fn new(source: &gtk::ScrolledWindow, panels: &[&gtk::Widget]) -> Self {
        let widget: Self = glib::Object::new();
        widget.set_can_target(false);
        let paintable = gtk::WidgetPaintable::new(Some(source));
        let weak = widget.downgrade();
        paintable.connect_invalidate_contents(move |_| {
            if let Some(widget) = weak.upgrade() {
                widget.queue_draw();
            }
        });
        widget.imp().source.replace(Some(paintable));
        widget
            .imp()
            .panels
            .replace(panels.iter().map(|panel| panel.downgrade()).collect());
        widget
    }
}

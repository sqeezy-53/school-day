use adw::prelude::*;
use gtk::{cairo, gdk, gio, glib, graphene};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::lessons::Lessons;
use crate::{column, notify, storage, text};

const PROGRESS_FILE: &str = "tutorial.json";
const CARD_WIDTH: i32 = 304;
const EDGE: i32 = 12;
const GAP: i32 = 12;

#[derive(Clone, Copy)]
pub(crate) enum Anchor {
    Date,
    AddLesson,
    AddHomework,
    Theme,
    EditLesson,
    Day,
}

impl Anchor {
    fn class(self) -> &'static str {
        match self {
            Self::Date => "tour-date",
            Self::AddLesson => "tour-add-lesson",
            Self::AddHomework => "tour-add-homework",
            Self::Theme => "tour-theme",
            Self::EditLesson => "tour-edit-lesson",
            Self::Day => "tour-day",
        }
    }

    pub(crate) fn mark(self, widget: &impl IsA<gtk::Widget>) {
        widget.add_css_class(self.class());
    }

    fn find(self, widget: &gtk::Widget) -> Option<gtk::Widget> {
        if !widget.is_visible() || !widget.is_child_visible() {
            return None;
        }
        if widget.has_css_class(self.class()) {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = self.find(&current) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
}

struct Step {
    tab: usize,
    anchor: Anchor,
    title: &'static str,
    description: &'static str,
}

const STEPS: &[Step] = &[
    Step {
        tab: 0,
        anchor: Anchor::Date,
        title: "Выбор даты",
        description: "Нажмите на дату, чтобы открыть календарь.",
    },
    Step {
        tab: 0,
        anchor: Anchor::AddLesson,
        title: "Новый урок",
        description: "Добавьте урок на выбранный день.",
    },
    Step {
        tab: 0,
        anchor: Anchor::Day,
        title: "Настройки дня",
        description: "Здесь можно включить дистант или сократить уроки.",
    },
    Step {
        tab: 0,
        anchor: Anchor::EditLesson,
        title: "Редактор урока",
        description: "Здесь можно указать замену, отмену или вид работы.",
    },
    Step {
        tab: 1,
        anchor: Anchor::AddHomework,
        title: "Домашнее задание",
        description: "Добавьте задание к уроку или отдельно.",
    },
    Step {
        tab: 2,
        anchor: Anchor::Theme,
        title: "Внешний вид",
        description: "Здесь можно выбрать тему приложения.",
    },
];

struct ReturnPoint {
    tab: usize,
    scroll: f64,
    focus: Option<glib::WeakRef<gtk::Widget>>,
}

struct Practice {
    items: Lessons,
    schedule: gtk::Box,
    homework: gtk::Box,
    calendar: gtk::Calendar,
}

struct Tour {
    window: glib::WeakRef<adw::ApplicationWindow>,
    overlay: glib::WeakRef<gtk::Overlay>,
    scroll: glib::WeakRef<gtk::ScrolledWindow>,
    pages: glib::WeakRef<gtk::Stack>,
    status: glib::WeakRef<gtk::Label>,
    tabs: [gtk::ToggleButton; 3],
    screen: Cell<Option<usize>>,
    return_point: RefCell<Option<ReturnPoint>>,
    practice: RefCell<Option<Practice>>,
    remembered: Cell<bool>,
    highlight: gtk::DrawingArea,
    card: gtk::Box,
    kicker: gtk::Label,
    title: gtk::Label,
    description: gtk::Label,
    instructions_scroll: gtk::ScrolledWindow,
    back: gtk::Button,
    next: gtk::Button,
    collapsed_group: RefCell<Option<glib::WeakRef<adw::ExpanderRow>>>,
}

pub fn install(
    window: &adw::ApplicationWindow,
    overlay: &gtk::Overlay,
    scroll: &gtk::ScrolledWindow,
    pages: &gtk::Stack,
    tabs: [gtk::ToggleButton; 3],
    status: &gtk::Label,
    offer_on_start: bool,
) {
    let progress_on_disk = storage::load_json(PROGRESS_FILE, |_: &mut bool| Ok(()));
    let highlight = gtk::DrawingArea::new();
    highlight.set_visible(false);
    overlay.add_overlay(&highlight);

    let card = column(5);
    card.add_css_class("tutorial-card");
    card.set_visible(false);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = text("", "tutorial-title");
    title.set_hexpand(true);
    let kicker = text("", "tutorial-kicker");
    kicker.set_wrap(false);
    kicker.set_valign(gtk::Align::Center);
    header.append(&title);
    header.append(&kicker);
    let description = text("", "tutorial-description");
    let instructions = column(3);
    instructions.append(&header);
    instructions.append(&description);
    let instructions_scroll = gtk::ScrolledWindow::new();
    instructions_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    instructions_scroll.set_overlay_scrolling(false);
    instructions_scroll.set_propagate_natural_height(true);
    instructions_scroll.set_min_content_height(32);
    instructions_scroll.set_vexpand(true);
    instructions_scroll.set_child(Some(&instructions));
    card.append(&instructions_scroll);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.add_css_class("tutorial-actions");
    let skip = gtk::Button::with_label("Пропустить");
    skip.add_css_class("flat");
    let spacer = column(0);
    spacer.set_hexpand(true);
    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.set_tooltip_text(Some("Назад"));
    back.add_css_class("flat");
    let next = gtk::Button::with_label("Далее");
    next.add_css_class("suggested-action");
    actions.append(&skip);
    actions.append(&spacer);
    actions.append(&back);
    actions.append(&next);
    card.append(&actions);
    overlay.add_overlay(&card);
    overlay.set_measure_overlay(&card, true);

    let tour = Rc::new(Tour {
        window: window.downgrade(),
        overlay: overlay.downgrade(),
        scroll: scroll.downgrade(),
        pages: pages.downgrade(),
        status: status.downgrade(),
        tabs,
        screen: Cell::new(None),
        return_point: RefCell::new(None),
        practice: RefCell::new(None),
        remembered: Cell::new(matches!(progress_on_disk, Ok(Some(true)))),
        highlight,
        card,
        kicker,
        title,
        description,
        instructions_scroll,
        back,
        next,
        collapsed_group: RefCell::default(),
    });

    let weak = Rc::downgrade(&tour);
    overlay.connect_get_child_position(move |overlay, child| {
        let tour = weak.upgrade()?;
        (child == tour.card.upcast_ref::<gtk::Widget>()).then(|| tour.place_card(overlay))
    });

    let weak = Rc::downgrade(&tour);
    tour.highlight.set_draw_func(move |_, context, _, _| {
        if let Some(tour) = weak.upgrade() {
            tour.draw(context);
        }
    });

    let click = gtk::GestureClick::new();
    click.set_button(1);
    let weak = Rc::downgrade(&tour);
    click.connect_released(move |_, _, x, y| {
        let Some(tour) = weak.upgrade() else {
            return;
        };
        if let Some(bounds) = tour
            .overlay
            .upgrade()
            .and_then(|overlay| tour.target_bounds(&overlay))
            && bounds.contains_point(&graphene::Point::new(x as f32, y as f32))
        {
            tour.try_target();
        }
    });
    tour.highlight.add_controller(click);

    let weak = Rc::downgrade(&tour);
    scroll.vadjustment().connect_value_changed(move |_| {
        if let Some(tour) = weak.upgrade().filter(|tour| tour.screen.get().is_some()) {
            tour.refresh_highlight();
        }
    });

    let weak = Rc::downgrade(&tour);
    skip.connect_clicked(move |_| {
        if let Some(tour) = weak.upgrade() {
            tour.finish();
        }
    });
    let weak = Rc::downgrade(&tour);
    tour.back.connect_clicked(move |_| {
        if let Some(tour) = weak.upgrade() {
            tour.previous();
        }
    });
    let weak = Rc::downgrade(&tour);
    tour.next.connect_clicked(move |_| {
        if let Some(tour) = weak.upgrade() {
            tour.advance();
        }
    });

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(&tour);
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        let Some(tour) = weak.upgrade().filter(|tour| tour.screen.get().is_some()) else {
            return glib::Propagation::Proceed;
        };
        if modifiers.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        ) {
            return glib::Propagation::Proceed;
        }
        if let Some(focus) = tour
            .window
            .upgrade()
            .and_then(|window| GtkWindowExt::focus(&window))
            && !focus.is_ancestor(&tour.card)
            && focus != tour.card.clone().upcast::<gtk::Widget>()
        {
            return glib::Propagation::Proceed;
        }
        match key {
            gdk::Key::Escape => tour.finish(),
            gdk::Key::Left => tour.previous(),
            gdk::Key::Right => tour.advance(),
            gdk::Key::F2 => tour.try_target(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    window.add_controller(keys);

    if offer_on_start && matches!(progress_on_disk, Ok(None | Some(false))) {
        let weak = Rc::downgrade(&tour);
        glib::idle_add_local_once(move || {
            if let Some(tour) = weak.upgrade() {
                tour.open(0);
            }
        });
    }
    let action = gio::SimpleAction::new("tutorial", None);
    action.connect_activate(move |_, _| {
        if tour.screen.get().is_none() {
            tour.open(0);
        }
    });
    window.add_action(&action);
}

impl Tour {
    fn open(self: &Rc<Self>, screen: usize) {
        if self.screen.get().is_some() {
            return;
        }
        let (Some(window), Some(root)) = (
            self.window.upgrade(),
            self.overlay.upgrade().and_then(|overlay| overlay.child()),
        ) else {
            return;
        };
        let tab = self
            .tabs
            .iter()
            .position(|tab| tab.is_active())
            .unwrap_or(0);
        let focus = GtkWindowExt::focus(&window).map(|widget| widget.downgrade());
        let scroll = self
            .scroll
            .upgrade()
            .map_or(0.0, |scroll| scroll.vadjustment().value());
        *self.return_point.borrow_mut() = Some(ReturnPoint { tab, scroll, focus });
        root.set_can_focus(false);
        self.highlight.set_visible(true);
        self.card.set_visible(true);
        self.show(screen);
    }

    fn show(self: &Rc<Self>, screen: usize) {
        self.restore_group();
        self.start_practice();
        self.screen.set(Some(screen));
        let step = self.step(screen);
        let last = screen + 1 == STEPS.len();
        self.tabs[step.tab].set_active(true);
        self.prepare_exercise(step.anchor);
        if let Some(pages) = self.pages.upgrade()
            && let Some(practice) = self.practice.borrow().as_ref()
        {
            match step.tab {
                0 => pages.set_visible_child(&practice.schedule),
                1 => pages.set_visible_child(&practice.homework),
                _ => {
                    pages.set_visible_child_name("settings");
                    if let Some(settings) = pages.visible_child() {
                        let _ = settings.activate_action("school-settings.home", None);
                    }
                }
            }
        }
        if let Some(group) = self
            .target()
            .and_downcast::<adw::ExpanderRow>()
            .filter(|group| group.is_expanded())
        {
            *self.collapsed_group.borrow_mut() = Some(group.downgrade());
            group.set_expanded(false);
        }
        if let Some(scroll) = self.scroll.upgrade() {
            scroll.vadjustment().set_value(0.0);
        }
        self.kicker
            .set_text(&format!("{} / {}", screen + 1, STEPS.len()));
        self.title.set_text(step.title);
        self.description.set_text(step.description);
        self.instructions_scroll.vadjustment().set_value(0.0);
        self.back.set_visible(screen > 0);
        self.next.set_label(if last { "Готово" } else { "Далее" });
        self.refresh_highlight();
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(120), move || {
            if let Some(tour) = weak
                .upgrade()
                .filter(|tour| tour.screen.get() == Some(screen))
            {
                tour.reveal_target();
                if tour.target().is_none() {
                    tour.description
                        .set_text("Элемент сейчас скрыт. Продолжите кнопкой «Далее».");
                }
            }
        });
        self.next.grab_focus();
    }

    fn step(&self, index: usize) -> &Step {
        &STEPS[index]
    }

    fn previous(self: &Rc<Self>) {
        if let Some(index) = self.screen.get().filter(|&i| i > 0) {
            self.show(index - 1);
        }
    }

    fn advance(self: &Rc<Self>) {
        match self.screen.get() {
            Some(index) if index + 1 < STEPS.len() => self.show(index + 1),
            Some(_) => self.finish(),
            None => {}
        }
    }

    fn start_practice(self: &Rc<Self>) {
        if self.practice.borrow().is_some() {
            return;
        }
        let (Some(pages), Some(status)) = (self.pages.upgrade(), self.status.upgrade()) else {
            return;
        };
        let items = Lessons::practice(|| {});
        let (schedule, calendar) = crate::schedule::schedule_page(&status, items.clone(), None);
        let homework = crate::homework::homework_page(&status, items.clone(), None);
        pages.add_child(&schedule);
        pages.add_child(&homework);
        *self.practice.borrow_mut() = Some(Practice {
            items,
            schedule,
            homework,
            calendar,
        });
    }

    fn prepare_exercise(&self, anchor: Anchor) {
        let practice = self.practice.borrow();
        let Some(practice) = practice.as_ref() else {
            return;
        };
        if matches!(anchor, Anchor::Day | Anchor::EditLesson) {
            let has_lesson = !practice.items.borrow().is_empty();
            if !has_lesson
                && let Some(tomorrow) = chrono::Local::now().date_naive().succ_opt()
                && let Ok(lesson) = crate::lessons::Lesson::new(
                    "География",
                    tomorrow,
                    "10:00".parse().unwrap(),
                    "10:40".parse().unwrap(),
                    "12",
                )
            {
                let _ = practice.items.store_lesson(lesson, None);
            }
            let first = practice.items.borrow().first().map(|item| item.date);
            if let Some(date) = first {
                crate::schedule::select_date(&practice.calendar, date);
            }
        }
    }

    fn try_target(&self) {
        let Some(index) = self.screen.get() else {
            return;
        };
        let anchor = self.step(index).anchor;
        let Some(page) = self.pages.upgrade().and_then(|pages| pages.visible_child()) else {
            return;
        };
        let Some(target) = anchor.find(&page) else {
            return;
        };
        match anchor {
            Anchor::Date | Anchor::Day => {
                if let Ok(menu) = target.downcast::<gtk::MenuButton>() {
                    menu.popup();
                }
            }
            Anchor::AddLesson | Anchor::EditLesson | Anchor::AddHomework => {
                if let Ok(button) = target.downcast::<gtk::Button>() {
                    button.emit_clicked();
                }
            }
            Anchor::Theme => {}
        }
    }

    fn restore_group(&self) {
        if let Some(group) = self
            .collapsed_group
            .borrow_mut()
            .take()
            .and_then(|weak| weak.upgrade())
        {
            group.set_expanded(true);
        }
    }

    fn finish(&self) {
        let Some(point) = self.return_point.borrow_mut().take() else {
            return;
        };
        self.screen.set(None);
        self.restore_group();
        self.card.set_visible(false);
        self.highlight.set_visible(false);
        if let Some(root) = self.overlay.upgrade().and_then(|overlay| overlay.child()) {
            root.set_can_focus(true);
        }
        self.tabs[point.tab].set_active(true);
        if let Some(pages) = self.pages.upgrade() {
            pages.set_visible_child_name(["schedule", "homework", "settings"][point.tab]);
            if let Some(practice) = self.practice.borrow_mut().take() {
                pages.remove(&practice.schedule);
                pages.remove(&practice.homework);
            }
        }
        if let Some(status) = self.status.upgrade() {
            status.set_visible(false);
        }
        if let Some(scroll) = self.scroll.upgrade() {
            let scroll = scroll.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(scroll) = scroll.upgrade() {
                    scroll.vadjustment().set_value(point.scroll);
                }
            });
        }
        if let Some(focus) = point.focus.and_then(|focus| focus.upgrade()) {
            focus.grab_focus();
        }
        if !self.remembered.get() {
            match storage::save_json(PROGRESS_FILE, &true) {
                Ok(()) => self.remembered.set(true),
                Err(error) => {
                    if let Some(status) = self.status.upgrade() {
                        notify(
                            &status,
                            &format!("Не удалось запомнить прохождение обучения: {error}"),
                        );
                    }
                }
            }
        }
    }

    fn target(&self) -> Option<gtk::Widget> {
        let step = self.step(self.screen.get()?);
        let page = self.pages.upgrade()?.visible_child()?;
        step.anchor.find(&page)
    }

    fn target_bounds(&self, overlay: &gtk::Overlay) -> Option<graphene::Rect> {
        let target = self.target()?;
        let bounds = target.compute_bounds(overlay)?;
        let viewport = self.scroll.upgrade()?.compute_bounds(overlay)?;
        (bounds.y() >= viewport.y()
            && bounds.y() + bounds.height() <= viewport.y() + viewport.height() + 1.0)
            .then_some(bounds)
    }

    fn refresh_highlight(&self) {
        if let Some(overlay) = self.overlay.upgrade() {
            overlay.queue_allocate();
        }
        self.highlight.queue_draw();
    }

    fn reveal_target(&self) {
        let (Some(scroll), Some(target)) = (self.scroll.upgrade(), self.target()) else {
            return;
        };
        if let Some(bounds) = target.compute_bounds(&scroll) {
            let viewport_height = scroll.height() as f32;
            if bounds.y() >= 0.0 && bounds.y() + bounds.height() > viewport_height - 48.0 {
                let adjustment = scroll.vadjustment();
                adjustment
                    .set_value(adjustment.value() + f64::from(bounds.y() - viewport_height / 3.0));
            }
        }
        self.refresh_highlight();
    }

    fn place_card(&self, overlay: &gtk::Overlay) -> gdk::Rectangle {
        let minimum_width = self.card.measure(gtk::Orientation::Horizontal, -1).0;
        let available_width = (overlay.width() - 2 * EDGE).max(minimum_width);
        let mut width = CARD_WIDTH.max(minimum_width).min(available_width);
        let available_height = overlay.height() - 2 * EDGE;
        let card_height = |width| {
            let (minimum, natural, _, _) = self.card.measure(gtk::Orientation::Vertical, width);
            natural.min(available_height.max(minimum))
        };
        let mut height = card_height(width);
        let mut x = (overlay.width() - width) / 2;
        let mut y = (overlay.height() - height) / 2;
        if let Some(target) = self.target_bounds(overlay) {
            x = (target.x() + target.width() / 2.0).round() as i32 - width / 2;
            let below = (target.y() + target.height()).ceil() as i32 + GAP;
            let above = target.y().floor() as i32 - GAP - height;
            y = below;
            if below + height + EDGE > overlay.height() {
                if above >= EDGE {
                    y = above;
                } else {
                    let right = (target.x() + target.width()).ceil() as i32 + GAP;
                    let left = target.x().floor() as i32 - GAP;
                    let room_on_right = overlay.width() - right - EDGE;
                    let room_on_left = left - EDGE;
                    let room = room_on_right.max(room_on_left);
                    if room >= minimum_width {
                        width = width.min(room);
                        height = card_height(width);
                        x = if room_on_right >= room_on_left {
                            right
                        } else {
                            left - width
                        };
                        y = (target.y() + target.height() / 2.0).round() as i32 - height / 2;
                    } else {
                        let above = target.y().floor() as i32 - GAP - EDGE;
                        let below_room = overlay.height() - EDGE - below;
                        let room = above.max(below_room);
                        let minimum = self.card.measure(gtk::Orientation::Vertical, width).0;
                        if room >= minimum {
                            height = room;
                            y = if below_room >= above { below } else { EDGE };
                        }
                    }
                }
            }
        }
        x = x.clamp(EDGE, (overlay.width() - width - EDGE).max(EDGE));
        y = y.clamp(EDGE, (overlay.height() - height - EDGE).max(EDGE));
        gdk::Rectangle::new(x, y, width, height)
    }

    fn draw(&self, context: &cairo::Context) {
        let Some(overlay) = self.overlay.upgrade() else {
            return;
        };
        if let Some(target) = self.target_bounds(&overlay) {
            context.set_source_rgb(0.42, 0.69, 1.0);
            context.set_line_width(2.0);
            spotlight_path(context, target);
            let _ = context.stroke();
        }
    }
}

fn spotlight_path(context: &cairo::Context, target: graphene::Rect) {
    let x = f64::from(target.x()) - 4.0;
    let y = f64::from(target.y()) - 4.0;
    let right = x + f64::from(target.width()) + 8.0;
    let bottom = y + f64::from(target.height()) + 8.0;
    let radius = 9.0;
    let quarter = std::f64::consts::FRAC_PI_2;
    context.new_sub_path();
    context.arc(right - radius, y + radius, radius, -quarter, 0.0);
    context.arc(right - radius, bottom - radius, radius, 0.0, quarter);
    context.arc(x + radius, bottom - radius, radius, quarter, 2.0 * quarter);
    context.arc(x + radius, y + radius, radius, 2.0 * quarter, 3.0 * quarter);
    context.close_path();
}

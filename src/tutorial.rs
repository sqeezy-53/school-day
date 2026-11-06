use adw::prelude::*;
use gtk::{cairo, gdk, gio, glib, graphene};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::lessons::Lessons;
use crate::{column, notify, storage, text};

const PROGRESS_FILE: &str = "tutorial.json";
const CARD_WIDTH: i32 = 360;
const EDGE: i32 = 14;
const GAP: i32 = 20;

const CSS: &str = r#"
.tutorial-card {
    padding: 20px;
    border-radius: 16px;
    border: 1px solid alpha(@window_fg_color, 0.16);
    background: mix(@window_bg_color, @window_fg_color, 0.08);
    color: @window_fg_color;
    box-shadow: 0 8px 24px alpha(black, 0.30);
}
.tutorial-icon {
    padding: 9px;
    border-radius: 10px;
    color: @window_fg_color;
    background: alpha(@window_fg_color, 0.10);
}
.tutorial-kicker { color: @window_fg_color; font-size: 12px; font-weight: 700; }
.tutorial-title { font-size: 21px; font-weight: 700; }
.tutorial-description { color: alpha(@window_fg_color, 0.82); }
.tutorial-actions button { border-radius: 8px; }
"#;

#[derive(Clone, Copy)]
pub(crate) enum Anchor {
    Date,
    Today,
    AddLesson,
    EditLesson,
    AddHomework,
    CompleteHomework,
    Theme,
    Bells,
}

impl Anchor {
    fn class(self) -> &'static str {
        match self {
            Self::Date => "tour-date",
            Self::Today => "tour-today",
            Self::AddLesson => "tour-add-lesson",
            Self::EditLesson => "tour-edit-lesson",
            Self::AddHomework => "tour-add-homework",
            Self::CompleteHomework => "tour-complete-homework",
            Self::Theme => "tour-theme",
            Self::Bells => "tour-bells",
        }
    }

    pub(crate) fn mark(self, widget: &impl IsA<gtk::Widget>) {
        widget.add_css_class(self.class());
    }

    fn find(self, widget: &gtk::Widget) -> Option<gtk::Widget> {
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
    anchor: Option<Anchor>,
    icon: &'static str,
    title: &'static str,
    description: &'static str,
}

const WELCOME: Step = Step {
    tab: 0,
    anchor: None,
    icon: "help-browser-symbolic",
    title: "Знакомство с программой",
    description: "Покажу где найти расписание, домашние задания и настройку звонков.",
};

const STEPS: &[Step] = &[
    Step {
        tab: 0,
        anchor: Some(Anchor::Date),
        icon: "x-office-calendar-symbolic",
        title: "Выберите день",
        description: "Нажмите на дату, чтобы открыть календарь.",
    },
    Step {
        tab: 0,
        anchor: Some(Anchor::Today),
        icon: "go-jump-today-symbolic",
        title: "Вернитесь к сегодняшнему дню",
        description: "Кнопка «Сегодня» возвращает к текущей дате.",
    },
    Step {
        tab: 0,
        anchor: Some(Anchor::AddLesson),
        icon: "list-add-symbolic",
        title: "Добавьте урок",
        description: "Нажмите «Добавить урок», заполните предмет и кабинет.",
    },
    Step {
        tab: 0,
        anchor: Some(Anchor::EditLesson),
        icon: "document-edit-symbolic",
        title: "Измените урок",
        description: "Рядом с сохранённым уроком есть карандаш. Нажмите его, чтобы изменить время, кабинет или записать домашнее задание.",
    },
    Step {
        tab: 1,
        anchor: None,
        icon: "document-edit-symbolic",
        title: "Добавить ДЗ",
        description: "Вкладка «ДЗ» собирает задания из расписания и позволяет их создавать. Здесь видно, к какому предмету и дате относится каждое задание.",
    },
    Step {
        tab: 1,
        anchor: Some(Anchor::AddHomework),
        icon: "list-add-symbolic",
        title: "Запишите задание",
        description: "Нажмите «Добавить ДЗ», выберите урок, введите задание и сохраните его.",
    },
    Step {
        tab: 1,
        anchor: Some(Anchor::CompleteHomework),
        icon: "object-select-symbolic",
        title: "Отметьте выполненное",
        description: "У каждого задания слева есть галочка. Нажмите её, чтобы отметить выполненное задание, или нажмите снова, чтобы снять отметку.",
    },
    Step {
        tab: 2,
        anchor: Some(Anchor::Theme),
        icon: "preferences-desktop-appearance-symbolic",
        title: "Выберите тему",
        description: "В настройках можно выбрать светлую или тёмную тему. В режиме Авто оформление меняется по заданному времени.",
    },
    Step {
        tab: 2,
        anchor: Some(Anchor::Bells),
        icon: "alarm-symbolic",
        title: "Настройте звонки",
        description: "В разделе «Звонки» задайте начало занятий, длительность уроков и перемен. Эти значения используются при добавлении урока.",
    },
];

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Welcome,
    Step(usize),
}

struct ReturnPoint {
    tab: usize,
    scroll: f64,
    focus: Option<glib::WeakRef<gtk::Widget>>,
}

struct Practice {
    items: Lessons,
    schedule: gtk::Box,
    homework: gtk::Box,
}

struct Tour {
    window: glib::WeakRef<adw::ApplicationWindow>,
    overlay: glib::WeakRef<gtk::Overlay>,
    scroll: glib::WeakRef<gtk::ScrolledWindow>,
    pages: glib::WeakRef<gtk::Stack>,
    status: glib::WeakRef<gtk::Label>,
    tabs: [gtk::ToggleButton; 3],
    screen: Cell<Option<Screen>>,
    return_point: RefCell<Option<ReturnPoint>>,
    practice: RefCell<Option<Practice>>,
    remembered: Cell<bool>,
    highlight: gtk::DrawingArea,
    card: gtk::Box,
    icon: gtk::Image,
    kicker: gtk::Label,
    title: gtk::Label,
    description: gtk::Label,
    hint: gtk::Label,
    progress: gtk::ProgressBar,
    back: gtk::Button,
    next: gtk::Button,
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
    let css = gtk::CssProvider::new();
    css.load_from_data(CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::prelude::WidgetExt::display(window),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let highlight = gtk::DrawingArea::new();
    highlight.set_visible(false);
    overlay.add_overlay(&highlight);

    let card = column(12);
    card.add_css_class("tutorial-card");
    card.set_visible(false);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let icon = gtk::Image::new();
    icon.set_pixel_size(20);
    icon.add_css_class("tutorial-icon");
    let kicker = text("", "tutorial-kicker");
    kicker.set_wrap(false);
    header.append(&icon);
    header.append(&kicker);
    card.append(&header);

    let title = text("", "tutorial-title");
    let description = text("", "tutorial-description");
    let hint = text("", "muted");
    card.append(&title);
    card.append(&description);
    card.append(&hint);

    let progress = gtk::ProgressBar::new();
    card.append(&progress);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.add_css_class("tutorial-actions");
    let skip = gtk::Button::with_label("Пропустить");
    skip.add_css_class("flat");
    let spacer = column(0);
    spacer.set_hexpand(true);
    let back = gtk::Button::with_label("Назад");
    let next = gtk::Button::with_label("Далее");
    next.add_css_class("suggested-action");
    actions.append(&skip);
    actions.append(&spacer);
    actions.append(&back);
    actions.append(&next);
    card.append(&actions);
    overlay.add_overlay(&card);

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
        icon,
        kicker,
        title,
        description,
        hint,
        progress,
        back,
        next,
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
        match key {
            gdk::Key::Escape => tour.finish(),
            gdk::Key::Left => tour.previous(),
            gdk::Key::Right => tour.advance(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    window.add_controller(keys);

    if offer_on_start && matches!(progress_on_disk, Ok(None | Some(false))) {
        let weak = Rc::downgrade(&tour);
        glib::idle_add_local_once(move || {
            if let Some(tour) = weak.upgrade() {
                tour.open(Screen::Welcome);
            }
        });
    }
    let action = gio::SimpleAction::new("tutorial", None);
    action.connect_activate(move |_, _| tour.open(Screen::Step(0)));
    window.add_action(&action);
}

impl Tour {
    fn open(self: &Rc<Self>, screen: Screen) {
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

    fn show(self: &Rc<Self>, screen: Screen) {
        if matches!(screen, Screen::Step(_)) {
            self.start_practice();
        }
        self.screen.set(Some(screen));
        let (step, position, previous, next) = match screen {
            Screen::Welcome => (
                &WELCOME,
                format!("Короткое обучение · {} шагов", STEPS.len()),
                false,
                "Начать",
            ),
            Screen::Step(index) => {
                let next = if index + 1 == STEPS.len() {
                    "Готово"
                } else {
                    "Далее"
                };
                (
                    &STEPS[index],
                    format!("Шаг {} из {}", index + 1, STEPS.len()),
                    index > 0,
                    next,
                )
            }
        };
        self.tabs[step.tab].set_active(true);
        if let Some(pages) = self.pages.upgrade()
            && let Some(practice) = self.practice.borrow().as_ref()
        {
            match step.tab {
                0 => pages.set_visible_child(&practice.schedule),
                1 => pages.set_visible_child(&practice.homework),
                _ => pages.set_visible_child_name("settings"),
            }
        }
        if let Some(scroll) = self.scroll.upgrade() {
            scroll.vadjustment().set_value(0.0);
        }
        self.icon.set_icon_name(Some(step.icon));
        self.kicker.set_text(&position);
        self.title.set_text(step.title);
        self.description.set_text(step.description);
        let exercise = self.exercise();
        let hint = match (screen, exercise) {
            (Screen::Welcome, _) => "Урок и ДЗ из обучения исчезнут после выхода.",
            (_, Some(Anchor::AddLesson)) => {
                "Введи, например, «Математика» и кабинет «12», затем сохрани урок."
            }
            (_, Some(Anchor::AddHomework)) => {
                "Для примера запиши «Решить задачу № 4» и сохрани задание."
            }
            (_, Some(Anchor::CompleteHomework)) => {
                "Нажми на выделенную галочку или кнопку «Попробовать»."
            }
            _ => "",
        };
        self.hint.set_text(hint);
        self.hint.set_visible(!hint.is_empty());
        self.back.set_visible(previous);
        self.next.set_label(if exercise.is_some() {
            "Попробовать"
        } else {
            next
        });
        self.progress.set_visible(matches!(screen, Screen::Step(_)));
        if let Screen::Step(index) = screen {
            self.progress
                .set_fraction((index + 1) as f64 / STEPS.len() as f64);
        }
        self.refresh_highlight();
        if step.anchor.is_some() {
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(150), move || {
                if let Some(tour) = weak
                    .upgrade()
                    .filter(|tour| tour.screen.get() == Some(screen))
                {
                    tour.reveal_target();
                }
            });
        }
        self.next.grab_focus();
    }

    fn previous(self: &Rc<Self>) {
        if let Some(Screen::Step(index)) = self.screen.get()
            && index > 0
        {
            self.show(Screen::Step(index - 1));
        }
    }

    fn advance(self: &Rc<Self>) {
        if self.exercise().is_some() {
            self.try_target();
            return;
        }
        match self.screen.get() {
            Some(Screen::Welcome) => self.show(Screen::Step(0)),
            Some(Screen::Step(index)) if index + 1 < STEPS.len() => {
                self.show(Screen::Step(index + 1));
            }
            Some(Screen::Step(_)) => self.finish(),
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
        let weak = Rc::downgrade(self);
        let items = Lessons::practice(move || {
            let Some(tour) = weak.upgrade() else {
                return;
            };
            let screen = tour.screen.get();
            let weak = Rc::downgrade(&tour);
            glib::idle_add_local_once(move || {
                if let Some(tour) = weak.upgrade()
                    && tour.screen.get() == screen
                    && tour.exercise().is_none()
                    && let Some(Screen::Step(index)) = screen
                    && matches!(
                        STEPS[index].anchor,
                        Some(Anchor::AddLesson | Anchor::AddHomework | Anchor::CompleteHomework)
                    )
                {
                    tour.advance();
                }
            });
        });
        let schedule = crate::schedule::schedule_page(&status, items.clone());
        let homework = crate::homework::homework_page(&status, items.clone());
        pages.add_child(&schedule);
        pages.add_child(&homework);
        *self.practice.borrow_mut() = Some(Practice {
            items,
            schedule,
            homework,
        });
    }

    fn exercise(&self) -> Option<Anchor> {
        let Screen::Step(index) = self.screen.get()? else {
            return None;
        };
        let anchor = STEPS[index].anchor?;
        let practice = self.practice.borrow();
        let items = practice.as_ref()?.items.borrow();
        match anchor {
            Anchor::AddLesson if items.is_empty() => Some(anchor),
            Anchor::AddHomework if items.iter().all(|lesson| lesson.homework.is_empty()) => {
                Some(anchor)
            }
            Anchor::CompleteHomework if items.iter().all(|lesson| !lesson.homework_completed) => {
                Some(anchor)
            }
            _ => None,
        }
    }

    fn try_target(&self) {
        let Some(Screen::Step(index)) = self.screen.get() else {
            return;
        };
        let (Some(anchor), Some(page)) = (
            STEPS[index].anchor,
            self.pages.upgrade().and_then(|pages| pages.visible_child()),
        ) else {
            return;
        };
        let Some(target) = anchor.find(&page) else {
            return;
        };
        match anchor {
            Anchor::Date => {
                if let Ok(menu) = target.downcast::<gtk::MenuButton>() {
                    menu.popup();
                }
            }
            Anchor::Today | Anchor::AddLesson | Anchor::EditLesson | Anchor::AddHomework => {
                if let Ok(button) = target.downcast::<gtk::Button>() {
                    button.emit_clicked();
                }
            }
            Anchor::CompleteHomework => {
                if let Ok(checkbox) = target.downcast::<gtk::CheckButton>() {
                    checkbox.set_active(!checkbox.is_active());
                }
            }
            Anchor::Theme | Anchor::Bells => {}
        }
    }

    fn finish(&self) {
        let Some(point) = self.return_point.borrow_mut().take() else {
            return;
        };
        self.screen.set(None);
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
        let Screen::Step(index) = self.screen.get()? else {
            return None;
        };
        let step = &STEPS[index];
        let target = step.anchor.and_then(|anchor| {
            let page = self.pages.upgrade()?.visible_child()?;
            anchor.find(&page)
        });
        Some(target.unwrap_or_else(|| self.tabs[step.tab].clone().upcast()))
    }

    fn target_bounds(&self, overlay: &gtk::Overlay) -> Option<graphene::Rect> {
        self.target()?.compute_bounds(overlay)
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
        let (_, mut height, _, _) = self.card.measure(gtk::Orientation::Vertical, width);
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
                        height = self.card.measure(gtk::Orientation::Vertical, width).1;
                        x = if room_on_right >= room_on_left {
                            right
                        } else {
                            left - width
                        };
                        y = (target.y() + target.height() / 2.0).round() as i32 - height / 2;
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
            if let Some(card) = self.card.compute_bounds(&overlay) {
                draw_pointer(context, target, card);
            }
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

fn draw_pointer(context: &cairo::Context, target: graphene::Rect, card: graphene::Rect) {
    let (tx, ty, tr, tb) = (
        target.x(),
        target.y(),
        target.x() + target.width(),
        target.y() + target.height(),
    );
    let (cx, cy, cr, cb) = (
        card.x(),
        card.y(),
        card.x() + card.width(),
        card.y() + card.height(),
    );
    let x = (tx + tr) / 2.0;
    let y = (ty + tb) / 2.0;
    let (tip, base, offset) = if cy >= tb + 12.0 || cb <= ty - 12.0 {
        let (tip_y, base_y) = if cy > tb {
            (tb + 7.0, cy + 1.0)
        } else {
            (ty - 7.0, cb - 1.0)
        };
        (
            (x, tip_y),
            (x.clamp(cx + 18.0, cr - 18.0), base_y),
            (6.0, 0.0),
        )
    } else if cx >= tr + 12.0 || cr <= tx - 12.0 {
        let (tip_x, base_x) = if cx > tr {
            (tr + 7.0, cx + 1.0)
        } else {
            (tx - 7.0, cr - 1.0)
        };
        (
            (tip_x, y),
            (base_x, y.clamp(cy + 18.0, cb - 18.0)),
            (0.0, 6.0),
        )
    } else {
        return;
    };
    context.move_to(tip.0.into(), tip.1.into());
    context.line_to((base.0 - offset.0).into(), (base.1 - offset.1).into());
    context.line_to((base.0 + offset.0).into(), (base.1 + offset.1).into());
    context.close_path();
    let _ = context.fill();
}

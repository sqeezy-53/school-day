use adw::prelude::*;
use chrono::{Datelike, NaiveDate, Weekday};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

mod art;
use art::{Artwork, Motif};
use std::fmt::Write;

#[derive(Clone, Copy)]
enum DateRule {
    Days(u32, u32, u32),
    Programmer,
    Libraries,
}

const fn day(month: u32, day: u32) -> DateRule {
    DateRule::Days(month, day, day)
}

impl DateRule {
    fn matches(self, date: NaiveDate) -> bool {
        match self {
            Self::Days(month, first, last) => {
                date.month() == month && (first..=last).contains(&date.day())
            }
            Self::Programmer => date.ordinal() == 256,
            Self::Libraries => {
                date.month() == 10
                    && (22..=28).contains(&date.day())
                    && date.weekday() == Weekday::Mon
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Holiday {
    date: DateRule,
    name: &'static str,
    color: &'static str,
    accent: &'static str,
    motif: &'static str,
    strong: bool,
    art: Motif,
}

const fn holiday(
    date: DateRule,
    name: &'static str,
    color: &'static str,
    accent: &'static str,
    motif: &'static str,
) -> Holiday {
    Holiday {
        date,
        name,
        color,
        accent,
        motif,
        strong: false,
        art: Motif::Confetti,
    }
}

const fn major(art: Motif, mut holiday: Holiday) -> Holiday {
    holiday.strong = true;
    holiday.art = art;
    holiday
}

const SNOW: &str = "M12 2v20 M3.3 7l17.4 10 M3.3 17l17.4-10 M9 4l3 3 3-3 M9 20l3-3 3 3 M3 11l4 1-1 4 M21 13l-4-1 1-4 M6 8l1 4-4 1 M18 16l-1-4 4-1";
const FIR: &str = "M12 2 7 9h3l-5 6h4l-5 5h16l-5-5h4l-5-6h3L12 2z M12 20v2";
const BOOK: &str = "M12 5C9 3 5 3 2 5v14c4-2 7-2 10 0 3-2 6-2 10 0V5c-3-2-7-2-10 0z M12 5v14 M5 8c2-1 3-1 5 0 M14 8c2-1 3-1 5 0";
const STUDENT: &str = "M2 9 12 4l10 5-10 5L2 9z M6 11v6c3 3 9 3 12 0v-6 M22 9v8";
const ATOM: &str = "M21 12c0 2-4 4-9 4s-9-2-9-4 4-4 9-4 9 2 9 4z M16 4c2 1 2 5-1 10s-6 7-7 6c-2-1-2-5 1-10s6-7 7-6z M8 4c2-1 5 1 7 6s3 9 1 10c-2 1-5-1-7-6S6 5 8 4z";
const HEART: &str = "M12 21 4 13C-3 6 7 0 12 7c5-7 15-1 8 6z";
const LETTER: &str = "M3 19 9 5l6 14 M5 14h8 M21 11v8 M21 13c-5-4-8 5-3 6 2 0 3-1 3-2";
const STAR: &str = "M12 2l3 6 7 1-5 5 1 7-6-3-6 3 1-7-5-5 7-1z";
const FLOWER: &str =
    "M12 9c-5-8 5-8 2-1 8-5 9 4 1 5 8 4-1 9-3 2-4 8-11 1-4-2-8 0-5-10 2-5z M12 11h.1";
const SUN: &str = "M18 12a6 6 0 1 1-12 0 6 6 0 1 1 12 0z M12 2v2 M12 20v2 M2 12h2 M20 12h2 M5 5l2 2 M17 17l2 2 M5 19l2-2 M17 7l2-2";
const CONFETTI: &str = "M3 20 7 9l8 8-12 3z M11 6l1-3 M17 10l4-2 M15 5l2-2 M19 14l2 1 M20 3h1";
const PLANET: &str =
    "M19 12a7 7 0 1 1-14 0 7 7 0 1 1 14 0z M6 10C-8 20 12 19 21 11c3-3 0-4-3-4 M20 2v3 M18.5 3.5h3";
const LEAF: &str = "M4 20c4-7 8-9 14-13 M6 16c-4-10 9-11 14-12 0 11-4 14-14 12z";
const FRAME: &str = "M3 3h18v18H3z M6 6h12v12H6z M6 15l4-4 3 3 2-2 3 3 M15 9h.1";
const PEN: &str = "M4 21l2-7L17 3l4 4-11 11-6 3z M14 6l4 4 M6 14l4 4";
const BALLOON: &str = "M18 9c0 4-3 7-6 7s-6-3-6-7 3-6 6-6 6 2 6 6z M12 16l-2 3h4l-2-3c2 3-2 3-1 4";
const FLAG: &str = "M4 22V3h16v12H4 M4 7h16 M4 11h16";
const FRIENDS: &str = "M11 8a3 3 0 1 1-6 0 3 3 0 1 1 6 0z M20 8a3 3 0 1 1-6 0 3 3 0 1 1 6 0z M2 20v-2a6 6 0 0 1 12 0v2 M16 13a6 6 0 0 1 6 6v1";
const FILM: &str = "M3 4h18v16H3z M7 4v16 M17 4v16 M3 8h4 M17 8h4 M3 16h4 M17 16h4 M7 12h10";
const CODE: &str = "m7 7-5 5 5 5 M17 7l5 5-5 5 M14 5l-4 14";
const BIRD: &str =
    "M3 15c5 0 7-2 9-5-3-1-5-3-5-7 4 0 7 2 9 7 1-3 4-3 5-1l2 1-3 2c-2 6-7 7-12 6l-4 3 1-4z";
const MOUNTAIN: &str = "m2 20 7-13 5 8 3-5 5 10z M7 11l2 2 2-2 M18 4h.1";
const MUSIC: &str =
    "M9 18V5l12-3v13 M9 8l12-3 M9 17c-4-2-8 2-5 4 2 1 5-1 5-3 M21 14c-4-2-8 2-5 4 2 1 5-1 5-3";
const PAW: &str = "M7 17c0-2 3-5 5-5s5 3 5 5c0 4-4 1-5 1s-5 3-5-1z M5 11a2 2 0 1 1 0-4 2 2 0 1 1 0 4z M10 8a2 2 0 1 1 0-4 2 2 0 1 1 0 4z M15 8a2 2 0 1 1 0-4 2 2 0 1 1 0 4z M20 11a2 2 0 1 1 0-4 2 2 0 1 1 0 4z";
const PUMPKIN: &str = "M12 7c0-3 2-4 4-4 M12 8C-2 2-2 23 12 21 26 23 26 2 12 8z M8 11l-2 3h4z M16 11l-2 3h4z M8 17c3 2 5 2 8 0 M2 3l2 1 1-1 1 1 2-1-1 2-2 1-2-1z";

const HOLIDAYS: &[Holiday] = &[
    major(
        Motif::Winter,
        holiday(
            DateRule::Days(1, 1, 3),
            "Новый год",
            "689fde",
            "c39a46",
            SNOW,
        ),
    ),
    holiday(day(1, 7), "Рождество", "367951", "b8903f", FIR),
    holiday(
        day(1, 24),
        "Международный день образования",
        "397dc7",
        "608fc7",
        BOOK,
    ),
    holiday(day(1, 25), "День студента", "845ba9", "806dc3", STUDENT),
    holiday(day(2, 8), "День российской науки", "626fc0", "795cb6", ATOM),
    holiday(
        day(2, 14),
        "День святого Валентина",
        "c65275",
        "b43857",
        HEART,
    ),
    holiday(
        day(2, 21),
        "Международный день родного языка",
        "3c918f",
        "31898d",
        LETTER,
    ),
    holiday(
        day(2, 23),
        "День защитника Отечества",
        "477147",
        "a98030",
        STAR,
    ),
    holiday(
        day(3, 8),
        "Международный женский день",
        "a16cba",
        "c0638e",
        FLOWER,
    ),
    holiday(
        day(3, 20),
        "Международный день счастья",
        "d09837",
        "b97c21",
        SUN,
    ),
    holiday(day(4, 1), "День смеха", "aa79af", "438d9c", CONFETTI),
    major(
        Motif::Space,
        holiday(day(4, 12), "День космонавтики", "3d60ab", "8e62d4", PLANET),
    ),
    holiday(day(4, 22), "День Земли", "509b70", "418fac", LEAF),
    holiday(day(4, 23), "Всемирный день книги", "a47756", "ba8a36", BOOK),
    holiday(
        day(5, 1),
        "Праздник Весны и Труда",
        "5c9b62",
        "8b9d40",
        FLOWER,
    ),
    major(
        Motif::Remembrance,
        holiday(day(5, 9), "День Победы", "893c58", "ba8b42", STAR),
    ),
    holiday(
        day(5, 18),
        "Международный день музеев",
        "a88c63",
        "af813b",
        FRAME,
    ),
    holiday(
        day(5, 24),
        "День славянской письменности и культуры",
        "89465e",
        "a46747",
        PEN,
    ),
    major(
        Motif::Kites,
        holiday(day(6, 1), "День защиты детей", "58a2c9", "be8b32", BALLOON),
    ),
    holiday(
        day(6, 5),
        "Всемирный день окружающей среды",
        "46895d",
        "438e62",
        LEAF,
    ),
    holiday(day(6, 6), "День русского языка", "527dbb", "4567b2", LETTER),
    holiday(day(6, 12), "День России", "507bc2", "bf4c57", FLAG),
    holiday(
        day(7, 8),
        "День семьи, любви и верности",
        "639372",
        "bca150",
        FLOWER,
    ),
    holiday(
        day(7, 30),
        "Международный день дружбы",
        "c69e4d",
        "c47b38",
        FRIENDS,
    ),
    holiday(
        day(8, 12),
        "Международный день молодёжи",
        "916bc4",
        "519bd2",
        CONFETTI,
    ),
    holiday(
        day(8, 22),
        "День Государственного флага России",
        "547ebc",
        "b94d5d",
        FLAG,
    ),
    holiday(
        day(8, 27),
        "День российского кино",
        "575362",
        "b89451",
        FILM,
    ),
    major(
        Motif::School,
        holiday(day(9, 1), "День знаний", "cd8641", "ab6834", BOOK),
    ),
    holiday(
        day(9, 8),
        "Международный день грамотности",
        "4f83bf",
        "4187ad",
        LETTER,
    ),
    holiday(
        DateRule::Programmer,
        "День программиста",
        "4b956e",
        "339897",
        CODE,
    ),
    holiday(
        day(9, 21),
        "Международный день мира",
        "70a7cc",
        "4d8cac",
        BIRD,
    ),
    holiday(
        day(9, 27),
        "Всемирный день туризма",
        "3f918c",
        "4f7f98",
        MOUNTAIN,
    ),
    holiday(
        day(10, 1),
        "Международный день музыки",
        "8a6bba",
        "925cb0",
        MUSIC,
    ),
    holiday(
        day(10, 4),
        "Всемирный день защиты животных",
        "5e9068",
        "698d49",
        PAW,
    ),
    major(
        Motif::Teachers,
        holiday(day(10, 5), "День учителя", "b39144", "bd7952", FLOWER),
    ),
    holiday(
        DateRule::Libraries,
        "Международный день школьных библиотек",
        "497b65",
        "ad9251",
        BOOK,
    ),
    major(
        Motif::Halloween,
        holiday(day(10, 31), "Хэллоуин", "704890", "da783e", PUMPKIN),
    ),
    holiday(
        day(11, 4),
        "День народного единства",
        "566eac",
        "a94d66",
        FRIENDS,
    ),
    holiday(
        day(11, 10),
        "Всемирный день науки",
        "8261ae",
        "6167b4",
        ATOM,
    ),
    major(
        Motif::Balloons,
        holiday(
            day(11, 20),
            "Всемирный день ребёнка",
            "5a9cc8",
            "bd933e",
            BALLOON,
        ),
    ),
    holiday(day(12, 5), "День добровольца", "508e68", "45857a", HEART),
    holiday(
        day(12, 12),
        "День Конституции России",
        "476191",
        "b45555",
        BOOK,
    ),
    holiday(
        day(12, 25),
        "Рождество по западной традиции",
        "a64853",
        "48835e",
        FIR,
    ),
    major(
        Motif::Winter,
        holiday(
            DateRule::Days(12, 29, 31),
            "Новый год",
            "6498d5",
            "7f9fbf",
            SNOW,
        ),
    ),
];

fn for_date(date: NaiveDate) -> Option<usize> {
    HOLIDAYS
        .iter()
        .position(|holiday| holiday.date.matches(date))
}

pub fn css() -> String {
    let mut css = include_str!("seasonal.css").to_owned();
    for (index, holiday) in HOLIDAYS.iter().enumerate() {
        let (strength, popover_strength) = if holiday.strong {
            ("0.22", "0.30")
        } else {
            ("0.10", "0.18")
        };
        writeln!(
            css,
            "window.holiday-{index} {{ --sd-hue: #{}; --sd-accent: #{}; --sd-strength: {strength}; --sd-popover-strength: {popover_strength}; }}",
            holiday.color, holiday.accent
        )
        .expect("Writing CSS to a String cannot fail");
    }
    css
}

pub(crate) fn name_on(date: NaiveDate) -> Option<&'static str> {
    for_date(date).map(|index| HOLIDAYS[index].name)
}

pub(crate) fn date_title(date: NaiveDate) -> Option<String> {
    name_on(date).map(|name| dated_name(name, date))
}

fn dated_name(name: &str, date: NaiveDate) -> String {
    const MONTHS: [&str; 12] = [
        "января",
        "февраля",
        "марта",
        "апреля",
        "мая",
        "июня",
        "июля",
        "августа",
        "сентября",
        "октября",
        "ноября",
        "декабря",
    ];
    format!("{} {} — {name}", date.day(), MONTHS[date.month0() as usize])
}

fn greeting(holiday: Holiday, date: NaiveDate, today: NaiveDate) -> String {
    if date == today && holiday.art != Motif::Remembrance {
        if holiday.art == Motif::Winter {
            "С Новым годом!".to_owned()
        } else {
            format!("Сегодня {}!", holiday.name)
        }
    } else {
        dated_name(holiday.name, date)
    }
}

struct Celebration {
    canvas: gtk::DrawingArea,
    corners: [gtk::DrawingArea; 2],
    banner: gtk::Revealer,
    label: gtk::Label,
    icon: gtk::Picture,
    artwork: Rc<RefCell<Option<Artwork>>>,
    burst: Rc<Cell<Option<f64>>>,
    tick: Rc<Cell<Option<gtk::TickCallbackId>>>,
    timeout: Rc<Cell<Option<gtk::glib::SourceId>>>,
}

impl Celebration {
    fn new(overlay: &gtk::Overlay) -> Self {
        let artwork = Rc::new(RefCell::new(None::<Artwork>));
        let burst = Rc::new(Cell::new(None));
        let canvas = gtk::DrawingArea::builder()
            .can_target(false)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .valign(gtk::Align::End)
            .height_request(46)
            .visible(false)
            .build();
        let (draw_art, draw_burst) = (artwork.clone(), burst.clone());
        canvas.set_draw_func(move |_, cr, width, height| {
            if let (Some(art), Some(progress)) = (&*draw_art.borrow(), draw_burst.get()) {
                art.draw_burst(cr, width, height, progress);
            }
        });
        overlay.add_overlay(&canvas);
        let corners = [false, true].map(|right| {
            let corner = gtk::DrawingArea::builder()
                .can_target(false)
                .accessible_role(gtk::AccessibleRole::Presentation)
                .width_request(96)
                .height_request(110)
                .halign(if right {
                    gtk::Align::End
                } else {
                    gtk::Align::Start
                })
                .valign(if right {
                    gtk::Align::Start
                } else {
                    gtk::Align::End
                })
                .margin_start(12)
                .margin_end(12)
                .margin_top(65)
                .margin_bottom(57)
                .visible(false)
                .build();
            let artwork = artwork.clone();
            corner.set_draw_func(move |_, cr, _, _| {
                if let Some(art) = &*artwork.borrow() {
                    art.draw_corner(cr, right);
                }
            });
            overlay.add_overlay(&corner);
            corner
        });
        let adaptive = corners.each_ref().map(|corner| corner.downgrade());
        canvas.connect_resize(move |_, width, _| {
            // The content clamp occupies the center 680 px. Keep artwork in the side gutters.
            for corner in &adaptive {
                if let Some(corner) = corner.upgrade() {
                    corner.set_visible(width >= 904);
                }
            }
        });

        let icon = gtk::Picture::builder()
            .accessible_role(gtk::AccessibleRole::Presentation)
            .can_shrink(false)
            .build();
        let label = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(46)
            .build();
        let contents = gtk::Box::builder()
            .accessible_role(gtk::AccessibleRole::Status)
            .spacing(8)
            .css_classes(["holiday-greeting"])
            .build();
        contents.append(&icon);
        contents.append(&label);
        let banner = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(260)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_start(54)
            .margin_end(54)
            .margin_bottom(7)
            .can_target(false)
            .css_classes(["holiday-announcement"])
            .child(&contents)
            .build();
        let weak_icon = icon.downgrade();
        banner.connect_child_revealed_notify(move |banner| {
            if !banner.reveals_child()
                && !banner.is_child_revealed()
                && let Some(icon) = weak_icon.upgrade()
            {
                icon.set_paintable(gtk::gdk::Paintable::NONE);
            }
        });
        overlay.add_overlay(&banner);

        Self {
            canvas,
            corners,
            banner,
            label,
            icon,
            artwork,
            burst,
            tick: Rc::new(Cell::new(None)),
            timeout: Rc::new(Cell::new(None)),
        }
    }

    fn stop(&self) {
        if let Some(timeout) = self.timeout.take() {
            timeout.remove();
        }
        if let Some(tick) = self.tick.take() {
            tick.remove();
        }
        self.burst.set(None);
        if let Some(artwork) = self.artwork.borrow_mut().as_mut() {
            artwork.clear_particle();
        }
        self.canvas.queue_draw();
        self.banner.set_reveal_child(false);
    }

    fn set_art(&self, holiday: Option<Holiday>, animate: bool) {
        *self.artwork.borrow_mut() = holiday.map(|holiday| Artwork::new(holiday, animate));
        self.canvas.set_visible(holiday.is_some());
        for corner in &self.corners {
            corner.set_visible(holiday.is_some() && self.canvas.width() >= 904);
            corner.queue_draw();
        }
        self.canvas.queue_draw();
    }

    fn show(&self, holiday: Holiday, date: NaiveDate, animate: bool) {
        let today = chrono::Local::now().date_naive();
        let icon = art::icon(holiday.motif, holiday.accent, 20)
            .map(|image| gtk::gdk::Texture::for_pixbuf(&image));
        self.icon.set_paintable(icon.as_ref());
        self.icon.set_visible(icon.is_some());
        self.label.set_label(&greeting(holiday, date, today));
        self.banner.set_reveal_child(true);
        let (banner, timeout) = (self.banner.downgrade(), self.timeout.clone());
        self.timeout.set(Some(gtk::glib::timeout_add_local_once(
            Duration::from_millis(2600),
            move || {
                timeout.take();
                if let Some(banner) = banner.upgrade() {
                    banner.set_reveal_child(false);
                }
            },
        )));

        if animate {
            let (burst, tick) = (self.burst.clone(), self.tick.clone());
            let artwork = self.artwork.clone();
            let start = Cell::new(None);
            self.tick
                .set(Some(self.canvas.add_tick_callback(move |canvas, clock| {
                    let time = clock.frame_time();
                    let first = start.get().unwrap_or_else(|| {
                        start.set(Some(time));
                        time
                    });
                    let progress = (time - first) as f64 / 950_000.0;
                    let active = progress < 1.0 && animations_enabled(canvas);
                    burst.set(active.then_some(progress));
                    canvas.queue_draw();
                    if active {
                        gtk::glib::ControlFlow::Continue
                    } else {
                        if let Some(artwork) = artwork.borrow_mut().as_mut() {
                            artwork.clear_particle();
                        }
                        tick.take();
                        gtk::glib::ControlFlow::Break
                    }
                })));
        }
    }
}

fn animations_enabled(widget: &impl IsA<gtk::Widget>) -> bool {
    widget.settings().is_gtk_enable_animations()
}

pub(crate) fn inherit_palette(window: &adw::Window) {
    window.add_css_class("school-day-dialog");
    window.add_css_class("school-day");
    if let Some(parent) = window.transient_for() {
        for class in parent.css_classes() {
            if class == "holiday" || class.starts_with("holiday-") {
                window.add_css_class(&class);
            }
        }
    }
}

fn set_palette(window: &gtk::Window, previous: Option<usize>, next: Option<usize>) {
    if let Some(index) = previous {
        window.remove_css_class(&format!("holiday-{index}"));
    }
    if let Some(index) = next {
        window.add_css_class("holiday");
        window.add_css_class(&format!("holiday-{index}"));
    } else {
        window.remove_css_class("holiday");
    }
}

pub fn attach(window: &adw::ApplicationWindow, overlay: &gtk::Overlay, calendar: &gtk::Calendar) {
    window.add_css_class("school-day");
    let celebration = Rc::new(Celebration::new(overlay));
    let current = Cell::new(None::<usize>);
    let selected = Cell::new(None::<NaiveDate>);
    let seen = RefCell::new(VecDeque::new());
    let weak_window = window.downgrade();
    let change = celebration.clone();
    let update = move |calendar: &gtk::Calendar, announce: bool| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };
        let date = crate::schedule::selected_date(calendar);
        if selected.replace(Some(date)) == Some(date) {
            return;
        }
        change.stop();
        let next = for_date(date);
        let previous = current.replace(next);
        if previous != next {
            set_palette(window.upcast_ref(), previous, next);
            for dialog in gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Window>().ok())
            {
                if dialog.has_css_class("school-day")
                    && dialog.transient_for().as_ref() == Some(window.upcast_ref())
                {
                    set_palette(&dialog, previous, next);
                }
            }
        }
        let holiday = next.map(|index| HOLIDAYS[index]);
        let should_greet = announce && holiday.is_some() && {
            let now = Instant::now();
            let mut seen = seen.borrow_mut();
            seen.retain(|&(_, time)| now.duration_since(time) < Duration::from_secs(30));
            if seen.iter().any(|&(seen_date, _)| seen_date == date) {
                false
            } else {
                if seen.len() >= 12 {
                    seen.pop_front();
                }
                seen.push_back((date, now));
                true
            }
        };
        let animate = should_greet
            && holiday.is_some_and(|h| h.art != Motif::Remembrance)
            && animations_enabled(&window);
        if next != previous || should_greet {
            change.set_art(holiday, animate);
        }
        if should_greet && let Some(holiday) = holiday {
            change.show(holiday, date, animate);
        }
    };
    update(calendar, false);
    calendar.connect_date_notify(move |calendar| update(calendar, true));
    window.connect_destroy(move |_| celebration.stop());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(year: i32, month: u32, day: u32) -> Option<usize> {
        for_date(NaiveDate::from_ymd_opt(year, month, day).unwrap())
    }

    #[test]
    fn greeting_uses_the_selected_year_and_keeps_memorial_days_sober() {
        let today = NaiveDate::from_ymd_opt(2026, 4, 12).unwrap();
        let same_day_last_year = today.with_year(2025).unwrap();
        let space = HOLIDAYS[for_date(today).unwrap()];
        assert_eq!(greeting(space, today, today), "Сегодня День космонавтики!");
        assert_eq!(
            greeting(space, same_day_last_year, today),
            "12 апреля — День космонавтики"
        );
        let memorial = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        let remembrance = HOLIDAYS[for_date(memorial).unwrap()];
        assert!(remembrance.art == Motif::Remembrance);
        assert_eq!(
            greeting(remembrance, memorial, memorial),
            "9 мая — День Победы"
        );
        assert_eq!(
            name_on(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()),
            Some("Новый год")
        );
    }

    #[test]
    fn changes_only_on_listed_days_and_handles_year_end() {
        for (month, day) in [
            (10, 30),
            (11, 1),
            (12, 28),
            (1, 4),
            (4, 11),
            (4, 13),
            (8, 31),
            (9, 2),
        ] {
            assert_eq!(selected(2026, month, day), None, "{month}/{day}");
        }
        for (month, day) in [
            (10, 31),
            (12, 29),
            (12, 31),
            (1, 1),
            (1, 3),
            (4, 12),
            (9, 1),
        ] {
            let index = selected(2026, month, day).unwrap();
            assert!(HOLIDAYS[index].strong);
        }
        assert_eq!(selected(2026, 12, 29), selected(2026, 12, 31));
        assert_eq!(selected(2026, 1, 1), selected(2026, 1, 3));
    }

    #[test]
    fn programmer_and_library_days_follow_the_calendar() {
        assert_eq!(selected(2024, 9, 12), selected(2026, 9, 13));
        assert!(selected(2024, 9, 12).is_some());
        assert!(selected(2024, 9, 13).is_none());
        assert!(selected(2026, 9, 12).is_none());
        let libraries = selected(2025, 10, 27);
        assert!(libraries.is_some());
        assert_eq!(selected(2026, 10, 26), libraries);
        assert_eq!(selected(2029, 10, 22), libraries);
        assert!(selected(2026, 10, 27).is_none());
    }

    #[test]
    fn calendar_has_44_entries_with_no_overlaps_in_common_or_leap_years() {
        assert_eq!(HOLIDAYS.len(), 44);
        for year in 2000..2040 {
            let mut covered = 0;
            for date in NaiveDate::from_ymd_opt(year, 1, 1)
                .unwrap()
                .iter_days()
                .take_while(|d| d.year() == year)
            {
                let count = HOLIDAYS
                    .iter()
                    .filter(|holiday| holiday.date.matches(date))
                    .count();
                assert!(count <= 1, "{date}");
                covered += count;
            }
            assert_eq!(covered, 48);
        }
    }
}

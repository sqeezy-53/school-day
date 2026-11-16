use super::*;
use gtk::gdk::prelude::GdkCairoContextExt;
use gtk::gdk_pixbuf::Pixbuf;

const PUMPKIN_ART: &str = r#"
<path d='M17 13q1-8 7-10l3 4q-5 2-5 7' fill='#769359' stroke='#4e6a41'/>
<path d='M20 13C9 7 2 15 3 23c1 9 8 13 17 11 10 2 17-2 18-11 1-8-7-16-18-10Z' fill='#ed9350' stroke='#a95136'/>
<path d='M15 14q-6 9-1 17m11-17q6 9 1 17' stroke='#bc633b'/>
<path d='m12 22 4-5 2 5Zm12 0 2-5 4 5ZM13 25q7 5 15 0-2 6-8 6t-7-6Z' fill='#523249'/>
"#;
const BAT_ART: &str = r#"
<path d='M20 14c-6-6-11-7-18-5 3 2 5 5 6 9 3-2 6-1 9 3l3 3 3-3c3-4 6-5 9-3 1-4 3-7 6-9-7-2-12-1-18 5Z' fill='#8d73b4' stroke='#60467f'/>
<path d='m16 16 1-6 3 3 3-3 1 6-4 5Z' fill='#8d73b4'/>
"#;
const WEB_ART: &str = r#"
<path d='M2 2h35M2 2v35M2 2l27 27M2 2l34 14M2 2l14 34M2 12q4-2 8-2 0-4 2-8M2 20q8-4 13-5 1-5 5-13M2 29q12-3 19-8 5-7 8-19M2 36q15-1 25-9 8-10 9-25' stroke='#a393bb' stroke-width='.85'/>
"#;
const FIR_ART: &str = r#"
<path d='M4 31Q17 17 37 6' stroke='#528276' stroke-width='2'/>
<path d='M7 28 5 17m5 8V12m4 9 2-14m3 11 4-14m1 11 6-11M9 26l12 1m-8-4 14 1m-9-5 14 1m-9-5 14 1m-10-5 11 1' stroke='#599f88' stroke-width='1.5'/>
<path d='M8 23 7 15m9 3 3-9m3 10 10 2m-6-8 10-1' stroke='#a3c9b5' stroke-width='.9'/>
"#;
const BAUBLE_ART: &str = r#"
<path d='M20 1v9m-4 2V9h8v3' stroke='#c89b51' stroke-width='1.4'/>
<circle cx='20' cy='24' r='12' fill='#d88477' stroke='#aa564f'/>
<path d='M9 22q11 6 22 0M11 29q9 5 18 0' stroke='#f5ce9f' stroke-width='1.6'/>
<path d='M15 15q-4 2-5 6' stroke='#f4b6a2' stroke-width='1.8'/>
"#;
const ROCKET_ART: &str = r#"
<path d='m15 18-8 3-2 8 9-3m8-1-2 10-8 2 3-10' fill='#da8a76' stroke='#a95660'/>
<path d='M12 27Q13 9 34 5q1 19-15 25Z' fill='#c4d5ee' stroke='#6886af'/>
<path d='M27 7q1 5 7 7V5Z' fill='#d98376'/>
<circle cx='24' cy='17' r='4' fill='#6f9ebd' stroke='#4d7094'/>
<path d='m10 30-5 6m7-3-3 5' stroke='#e8a65e' stroke-width='1.6'/>
"#;
const PLANET_ART: &str = r#"
<circle cx='20' cy='19' r='13' fill='#bcabdf' stroke='#896bb4'/>
<path d='M8 16q15-1 23-6m-22 14q14-2 23-9' stroke='#9e85c7'/>
<path d='M7 21C-7 33 18 33 34 20c7-6 1-9-2-8' stroke='#e4af69' stroke-width='2'/>
"#;
const NOTEBOOK_ART: &str = r#"
<path d='M8 9h24v27H8q-3-1-3-4V12q0-3 3-3Z' fill='#f3deab' stroke='#9a754a'/>
<path d='M8 5h24v26H8q-2 0-3 2V9q0-4 3-4Z' fill='#82a685' stroke='#587b61'/>
<path d='M10 6v24m5-18h12v8H15Z' stroke='#d6e0aa'/>
<path d='M18 16h6' stroke='#d6e0aa'/>
"#;
const PENCIL_ART: &str = r#"
<path d='m6 28 21-23q2-2 4 0l3 3q2 2 0 4L12 34l-8 2Z' fill='#e3ac5b' stroke='#a67239'/>
<path d='m26 6 7 7m-11-3 7 7m-21 12 4 4' stroke='#a67239'/>
<path d='m6 28 6 6-8 2Z' fill='#edd6a9'/>
<path d='m5 33 2 2-3 1Z' fill='#594b51'/>
"#;
const BOUQUET_ART: &str = r#"
<path d='m20 37-8-24m8 24 8-23m-8 23 1-29m-2 20q-10 1-11-7 6-1 11 7Zm3-1q10 0 13-7-8-1-13 7Z' stroke='#689476'/>
<path d='M12 9c-3-7-8-2-6 2-5 2-1 8 3 6 4 5 10 1 7-3 5-3 0-8-4-5Zm17 1c3-6 8-2 6 2 5 2 1 7-3 5-4 5-9 1-6-3-4-3 0-7 3-4Z' fill='#df968c' stroke='#ac666e'/>
<path d='M21 4c-5-4-9 2-5 5-2 5 4 8 6 4 5 3 9-3 5-5 1-5-3-7-6-4Z' fill='#e4b169' stroke='#ad7942'/>
<path d='m15 30 12 3m-11 1 10-3' stroke='#bb785f' stroke-width='1.7'/>
"#;
const KITE_ART: &str = r#"
<path d='M22 3 37 17 16 29 8 10Z' fill='#77b8d5' stroke='#4e83a6'/>
<path d='M22 3 37 17l-21 12Z' fill='#f0be6b'/>
<path d='M22 3 16 29M8 10l29 7' stroke='#795e73' stroke-opacity='.7'/>
<path d='M16 29c-7 4-2 8-10 10' stroke='#857398'/>
<path d='m11 33 6-1-3 4-4-1-3 3v-5Z' fill='#d78192'/>
"#;
const BALLOONS_ART: &str = r#"
<path d='M12 22c4 5-3 10 8 15m9-15c-5 8 1 11-9 15m0-21c-4 8 4 14 0 21' stroke='#8c7893' stroke-width='.9'/>
<ellipse cx='12' cy='14' rx='7' ry='9' transform='rotate(-16 12 14)' fill='#e69a9f' stroke='#b16b7b'/>
<ellipse cx='28' cy='15' rx='7' ry='9' transform='rotate(16 28 15)' fill='#e9b971' stroke='#b98546'/>
<ellipse cx='20' cy='11' rx='7' ry='9' fill='#85b8d6' stroke='#5d8cab'/>
<path d='M9 9q-3 2-2 5m11-8q-3 2-3 5m12 1q-2 2-1 5' stroke='#fff' stroke-opacity='.65'/>
"#;
const SUN_ART: &str = r#"
<circle cx='20' cy='20' r='9' fill='#f2c572' stroke='#c58d46'/>
<path d='M20 4v3m0 26v3M4 20h3m26 0h3M9 9l2 2m18 18 2 2m0-22-2 2M11 29l-2 2' stroke='#d4a252' stroke-width='1.6'/>
"#;

type Piece<'a> = (&'a str, u32, u32, u32);
type Corners = (&'static [Piece<'static>], &'static [Piece<'static>]);

const HALLOWEEN: Corners = (
    &[(PUMPKIN_ART, 1, 37, 72), (BAT_ART, 47, 18, 41)],
    &[(WEB_ART, 34, 1, 57), (BAT_ART, 24, 47, 46)],
);
const WINTER: Corners = (
    &[(FIR_ART, 0, 30, 77), (BAUBLE_ART, 33, 59, 39)],
    &[(FIR_ART, 20, 0, 73), (BAUBLE_ART, 49, 34, 39)],
);
const SPACE: Corners = (&[(PLANET_ART, 3, 37, 69)], &[(ROCKET_ART, 20, 5, 66)]);
const SCHOOL: Corners = (
    &[(NOTEBOOK_ART, 0, 39, 63), (PENCIL_ART, 38, 40, 49)],
    &[(NOTEBOOK_ART, 37, 10, 54), (PENCIL_ART, 15, 49, 41)],
);
const TEACHERS: Corners = (
    &[(BOUQUET_ART, 0, 35, 70)],
    &[(NOTEBOOK_ART, 31, 10, 62), (PENCIL_ART, 27, 45, 42)],
);
const KITES: Corners = (
    &[(BALLOONS_ART, 1, 39, 69), (PENCIL_ART, 47, 63, 42)],
    &[(SUN_ART, 2, 8, 31), (KITE_ART, 22, 17, 73)],
);
const BALLOONS: Corners = (&[], &[(BALLOONS_ART, 22, 22, 65)]);

pub(super) fn decode_svg(svg: String) -> Option<Pixbuf> {
    let input = gtk::gio::MemoryInputStream::from_bytes(&gtk::glib::Bytes::from_owned(svg));
    Pixbuf::from_stream(&input, gtk::gio::Cancellable::NONE).ok()
}

fn composition(pieces: &[Piece<'_>], detail: Option<Piece<'_>>) -> Option<Pixbuf> {
    if pieces.is_empty() && detail.is_none() {
        return None;
    }
    let mut svg = String::from(
        "<svg xmlns='http://www.w3.org/2000/svg' width='192' height='220' viewBox='0 0 96 110' fill='none' stroke-linecap='round' stroke-linejoin='round'>",
    );
    for &(art, x, y, size) in pieces.iter().chain(detail.iter()) {
        let scale = f64::from(size) / 40.0;
        let _ = write!(
            svg,
            "<g transform='translate({x} {y}) scale({scale})'>{art}</g>"
        );
    }
    svg.push_str("</svg>");
    decode_svg(svg)
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Motif {
    #[default]
    Confetti,
    Halloween,
    Winter,
    Space,
    School,
    Teachers,
    Kites,
    Balloons,
    Remembrance,
}

pub(super) struct Artwork {
    left: Option<Pixbuf>,
    right: Option<Pixbuf>,
    particle: Option<Pixbuf>,
    colors: [gtk::gdk::RGBA; 2],
}

impl Artwork {
    pub fn new(holiday: Holiday, animate: bool) -> Self {
        let (left, right): Corners = match holiday.art {
            Motif::Halloween => HALLOWEEN,
            Motif::Winter => WINTER,
            Motif::Space => SPACE,
            Motif::School => SCHOOL,
            Motif::Teachers => TEACHERS,
            Motif::Kites => KITES,
            Motif::Balloons => BALLOONS,
            Motif::Remembrance | Motif::Confetti => (&[], &[]),
        };
        let detail = |path: &str| {
            format!(
                "<path transform='scale(1.667)' d='{path}' stroke='#{}' stroke-width='1.35'/>",
                holiday.accent
            )
        };
        let secondary = match holiday.art {
            Motif::Halloween | Motif::Space => Some(detail(STAR)),
            Motif::Winter => Some(detail(SNOW)),
            Motif::School => Some(detail(LEAF)),
            Motif::Confetti => Some(detail(holiday.motif)),
            _ => None,
        };
        let remembrance = (holiday.art == Motif::Remembrance).then(|| detail(STAR));
        let particle = if animate {
            match holiday.art {
                Motif::Winter => icon(SNOW, holiday.accent, 14),
                Motif::Space => icon(STAR, holiday.accent, 14),
                Motif::School => icon(LEAF, holiday.accent, 14),
                Motif::Teachers => icon(FLOWER, holiday.accent, 14),
                _ => None,
            }
        } else {
            None
        };
        Self {
            left: composition(left, remembrance.as_deref().map(|star| (star, 22, 48, 38))),
            right: composition(
                right,
                secondary.as_deref().map(|art| match holiday.art {
                    Motif::Winter => (art, 16, 67, 26),
                    Motif::Space => (art, 10, 58, 23),
                    Motif::School => (art, 64, 70, 24),
                    Motif::Halloween => (art, 75, 58, 15),
                    _ => (art, 46, 56, 43),
                }),
            ),
            particle,
            colors: [holiday.color, holiday.accent].map(|color| {
                gtk::gdk::RGBA::parse(format!("#{color}")).unwrap_or(gtk::gdk::RGBA::WHITE)
            }),
        }
    }

    pub fn clear_particle(&mut self) {
        self.particle = None;
    }

    pub fn draw_corner(&self, cr: &gtk::cairo::Context, right: bool) {
        let image = if right { &self.right } else { &self.left };
        if let Some(image) = image {
            draw_image(cr, image, 0.0, 0.0, 0.5, 0.90);
        }
    }

    pub fn draw_burst(&self, cr: &gtk::cairo::Context, width: i32, height: i32, progress: f64) {
        let (width, height) = (f64::from(width), f64::from(height));
        for i in 0..12 {
            let idx = f64::from(i / 2);
            let dir = if i % 2 == 0 { -1.0 } else { 1.0 };
            let spread = (width * 0.25).min(235.0);
            let x = width / 2.0 + dir * (spread * (0.66 + idx * 0.09) + 30.0 * progress);
            let y =
                height - 13.0 - (0.5 + idx * 0.08) * 30.0 * (progress * std::f64::consts::PI).sin();
            let alpha = (1.0 - progress) * 0.85;
            if let Some(image) = &self.particle {
                draw_image(cr, image, x, y, 0.75, alpha);
            } else {
                let color = self.colors[(i / 2) as usize % 2];
                cr.set_source_rgba(
                    f64::from(color.red()),
                    f64::from(color.green()),
                    f64::from(color.blue()),
                    alpha,
                );
                cr.arc(x, y, 1.6 + idx % 2.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            }
        }
    }
}

pub(super) fn icon(path: &str, accent: &str, size: u32) -> Option<Pixbuf> {
    decode_svg(format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{size}' height='{size}' viewBox='0 0 24 24'>\
         <path d='{path}' fill='none' stroke='#{accent}' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'/></svg>"
    ))
}

fn draw_image(cr: &gtk::cairo::Context, image: &Pixbuf, x: f64, y: f64, scale: f64, alpha: f64) {
    let _ = cr.save();
    cr.translate(x, y);
    cr.scale(scale, scale);
    cr.set_source_pixbuf(image, 0.0, 0.0);
    let _ = cr.paint_with_alpha(alpha);
    let _ = cr.restore();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_has_decodable_artwork() {
        for &holiday in HOLIDAYS {
            let artwork = Artwork::new(holiday, true);
            assert!(
                artwork.left.is_some() || artwork.right.is_some(),
                "{}",
                holiday.name
            );
            assert!(
                icon(holiday.motif, holiday.accent, 20).is_some(),
                "{}",
                holiday.name
            );
        }
    }
}
